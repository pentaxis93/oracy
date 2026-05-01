use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use caseless::Caseless;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{QueryBuilder, Row, SqlitePool};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use ulid::Ulid;
use unicode_normalization::UnicodeNormalization;

use crate::audio_hash::AUDIO_CONTENT_HASH_ALGORITHM_ID;
use crate::settings::DEFAULT_TRANSCRIPTION_MODEL;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct Storage {
    pool: SqlitePool,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database path has no parent directory: {0}")]
    MissingParent(PathBuf),
    #[error("database parent directory does not exist: {0}")]
    MissingParentDirectory(PathBuf),
    #[error("database parent path is not a directory: {0}")]
    ParentNotDirectory(PathBuf),
    #[error("database path is a directory: {0}")]
    PathIsDirectory(PathBuf),
    #[error("database parent directory is not writable: {path}: {source}")]
    ParentNotWritable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open database {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },
    #[error("failed to migrate database {path}: {source}")]
    Migrate {
        path: PathBuf,
        #[source]
        source: sqlx::migrate::MigrateError,
    },
    #[error(
        "unsupported audio content hash algorithm in database: expected {expected}, found {found}"
    )]
    UnsupportedAudioContentHashAlgorithm {
        expected: &'static str,
        found: String,
    },
    #[error("storage query failed: {0}")]
    Query(#[from] sqlx::Error),
    #[error("job is not eligible for transcript completion: {job_id}")]
    JobNotCompletable { job_id: String },
    #[error("stored timestamp is invalid: {0}")]
    InvalidTimestamp(#[from] time::error::Parse),
    #[error("timestamp could not be formatted: {0}")]
    FormatTimestamp(#[from] time::error::Format),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptJobOutcome {
    Created(TranscriptionJobRecord),
    Replayed(TranscriptionJobRecord),
    Conflict(SubmissionConflict),
    SessionNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateTagOutcome {
    Created(TagRecord),
    Existing(TagRecord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTagOutcome {
    Renamed(TagRecord),
    NotFound,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaceTranscriptTagsOutcome {
    Replaced,
    NotFound,
    DuplicateTagIds,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionConflict {
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPatch {
    pub transcription_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRecord {
    pub transcription_model: String,
}

#[derive(Debug, Clone)]
pub struct NewTranscriptionJob {
    pub api_key_id: String,
    pub idempotency_key: String,
    pub audio_sha256_hex: String,
    pub recorded_at: OffsetDateTime,
    pub session_id: Option<String>,
    pub language: Option<String>,
    pub accepted_audio_path: PathBuf,
    pub max_retries: i64,
    pub now: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewTag {
    pub id: String,
    pub api_key_id: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: String,
    pub api_key_id: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionJobRecord {
    pub id: String,
    pub api_key_id: String,
    pub idempotency_key: String,
    pub audio_sha256_hex: String,
    pub audio_content_hash_algorithm: String,
    pub recorded_at: OffsetDateTime,
    pub session_id: Option<String>,
    pub language: Option<String>,
    pub accepted_audio_path: PathBuf,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub retry_count: i64,
    pub max_retries: i64,
    pub next_attempt_at: Option<OffsetDateTime>,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub retryable_by_client: Option<bool>,
    pub transcript_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TranscriptMaterialization {
    pub transcript: NewTranscript,
    pub initial_version: NewTranscriptVersion,
    pub segments: Vec<NewSegment>,
    pub embedding: NewEmbedding,
}

#[derive(Debug, Clone)]
pub struct NewTranscript {
    pub id: String,
    pub audio_duration_seconds: f64,
    pub audio_format: String,
    pub audio_size_bytes: i64,
    pub transcript_language: Option<String>,
    pub model: String,
    pub processing_time_ms: i64,
    pub cost_cents: Option<i64>,
    pub created_at: OffsetDateTime,
    pub recorded_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewTranscriptVersion {
    pub id: String,
    pub transcript: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewSegment {
    pub id: String,
    pub position: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct NewEmbedding {
    pub model: String,
    pub vector: Vec<u8>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptRecord {
    pub id: String,
    pub api_key_id: String,
    pub current_version_id: String,
    pub transcript: String,
    pub audio_duration_seconds: f64,
    pub audio_format: String,
    pub audio_size_bytes: i64,
    pub transcript_language: Option<String>,
    pub model: String,
    pub processing_time_ms: i64,
    pub cost_cents: Option<i64>,
    pub created_at: OffsetDateTime,
    pub recorded_at: OffsetDateTime,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptVersionRecord {
    pub id: String,
    pub transcript_id: String,
    pub transcript: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptFilters {
    pub tag_ids: Vec<String>,
    pub session_id: Option<String>,
    pub recorded_after: Option<OffsetDateTime>,
    pub recorded_before: Option<OffsetDateTime>,
    pub created_after: Option<OffsetDateTime>,
    pub created_before: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentRecord {
    pub id: String,
    pub transcript_id: String,
    pub position: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRecord {
    pub transcript_id: String,
    pub model: String,
    pub vector: Vec<u8>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRecord {
    pub id: String,
    pub api_key_id: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub id: String,
    pub api_key_id: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

impl Storage {
    pub async fn connect(database_path: &Path) -> Result<Self, StorageError> {
        validate_database_path(database_path)?;

        let options = SqliteConnectOptions::new()
            .filename(database_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|source| StorageError::Open {
                path: database_path.to_path_buf(),
                source,
            })?;

        MIGRATOR
            .run(&pool)
            .await
            .map_err(|source| StorageError::Migrate {
                path: database_path.to_path_buf(),
                source,
            })?;
        ensure_supported_audio_content_hash_algorithm(&pool).await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn get_settings(&self, api_key_id: &str) -> Result<SettingsRecord, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT transcription_model
            FROM api_key_settings
            WHERE api_key_id = ?
            "#,
        )
        .bind(api_key_id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(settings_from_row(row)?),
            None => Ok(SettingsRecord {
                transcription_model: DEFAULT_TRANSCRIPTION_MODEL.to_owned(),
            }),
        }
    }

    pub async fn update_settings(
        &self,
        api_key_id: &str,
        patch: SettingsPatch,
        now: OffsetDateTime,
    ) -> Result<SettingsRecord, StorageError> {
        let Some(transcription_model) = patch.transcription_model else {
            return self.get_settings(api_key_id).await;
        };

        let updated_at = format_timestamp(now)?;
        sqlx::query(
            r#"
            INSERT INTO api_key_settings (api_key_id, transcription_model, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(api_key_id) DO UPDATE SET
                transcription_model = excluded.transcription_model,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(api_key_id)
        .bind(transcription_model)
        .bind(updated_at)
        .execute(&self.pool)
        .await?;

        self.get_settings(api_key_id).await
    }

    pub async fn accept_job(
        &self,
        input: NewTranscriptionJob,
    ) -> Result<AcceptJobOutcome, StorageError> {
        let mut tx = self.pool.begin().await?;
        let id = new_id();
        let now = format_timestamp(input.now)?;
        let recorded_at = format_timestamp(input.recorded_at)?;
        let accepted_audio_path = input.accepted_audio_path.to_string_lossy().into_owned();

        let result = sqlx::query(
            r#"
            INSERT INTO transcription_jobs (
                id, api_key_id, idempotency_key, audio_sha256_hex,
                audio_content_hash_algorithm, recorded_at, session_id, language,
                accepted_audio_path, status, created_at, updated_at, retry_count,
                max_retries
            )
            SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?, 0, ?
            WHERE ? IS NULL
                OR EXISTS (
                    SELECT 1 FROM sessions
                    WHERE api_key_id = ? AND id = ?
                )
            ON CONFLICT(api_key_id, idempotency_key) DO NOTHING
            "#,
        )
        .bind(&id)
        .bind(&input.api_key_id)
        .bind(&input.idempotency_key)
        .bind(&input.audio_sha256_hex)
        .bind(AUDIO_CONTENT_HASH_ALGORITHM_ID)
        .bind(recorded_at)
        .bind(&input.session_id)
        .bind(&input.language)
        .bind(accepted_audio_path)
        .bind(&now)
        .bind(&now)
        .bind(input.max_retries)
        .bind(&input.session_id)
        .bind(&input.api_key_id)
        .bind(&input.session_id)
        .execute(&mut *tx)
        .await?;

        let job = if result.rows_affected() == 1 {
            let row = sqlx::query(
                r#"
                SELECT * FROM transcription_jobs
                WHERE api_key_id = ? AND id = ?
                "#,
            )
            .bind(&input.api_key_id)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
            job_from_row(row)?
        } else {
            let row = sqlx::query(
                r#"
                SELECT * FROM transcription_jobs
                WHERE api_key_id = ? AND idempotency_key = ?
                "#,
            )
            .bind(&input.api_key_id)
            .bind(&input.idempotency_key)
            .fetch_optional(&mut *tx)
            .await?
            .map(job_from_row)
            .transpose()?;
            match row {
                Some(job) => job,
                None => {
                    tx.commit().await?;
                    return Ok(AcceptJobOutcome::SessionNotFound);
                }
            }
        };
        tx.commit().await?;

        if result.rows_affected() == 1 {
            Ok(AcceptJobOutcome::Created(job))
        } else if job.matches_submission(&input) {
            Ok(AcceptJobOutcome::Replayed(job))
        } else {
            Ok(AcceptJobOutcome::Conflict(SubmissionConflict {
                job_id: job.id,
            }))
        }
    }

    pub async fn find_job_by_idempotency_key(
        &self,
        api_key_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<TranscriptionJobRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT * FROM transcription_jobs
            WHERE api_key_id = ? AND idempotency_key = ?
            "#,
        )
        .bind(api_key_id)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await?;

        row.map(job_from_row).transpose()
    }

    pub async fn get_job(
        &self,
        api_key_id: &str,
        job_id: &str,
    ) -> Result<Option<TranscriptionJobRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT * FROM transcription_jobs
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(job_from_row).transpose()
    }

    pub async fn complete_job_with_transcript(
        &self,
        api_key_id: &str,
        job_id: &str,
        materialization: TranscriptMaterialization,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let transcript = materialization.transcript;
        let version = materialization.initial_version;
        let now = format_timestamp(transcript.created_at)?;

        let result = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET updated_at = ?
            WHERE api_key_id = ?
                AND id = ?
                AND status = 'processing'
                AND transcript_id IS NULL
            "#,
        )
        .bind(&now)
        .bind(api_key_id)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::JobNotCompletable {
                job_id: job_id.to_owned(),
            });
        }

        let transcript_session_id: Option<String> = sqlx::query(
            r#"
            SELECT sessions.id AS transcript_session_id
            FROM transcription_jobs
            LEFT JOIN sessions
                ON sessions.api_key_id = transcription_jobs.api_key_id
                AND sessions.id = transcription_jobs.session_id
            WHERE transcription_jobs.api_key_id = ?
                AND transcription_jobs.id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(job_id)
        .fetch_one(&mut *tx)
        .await?
        .try_get("transcript_session_id")?;

        sqlx::query(
            r#"
            INSERT INTO transcripts (
                id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
                transcript_language, model, processing_time_ms, cost_cents,
                created_at, recorded_at, session_id
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&transcript.id)
        .bind(api_key_id)
        .bind(transcript.audio_duration_seconds)
        .bind(&transcript.audio_format)
        .bind(transcript.audio_size_bytes)
        .bind(&transcript.transcript_language)
        .bind(&transcript.model)
        .bind(transcript.processing_time_ms)
        .bind(transcript.cost_cents)
        .bind(&now)
        .bind(format_timestamp(transcript.recorded_at)?)
        .bind(&transcript_session_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO transcript_versions (
                id, api_key_id, transcript_id, version_number, transcript, created_at
            )
            VALUES (?, ?, ?, 1, ?, ?)
            "#,
        )
        .bind(&version.id)
        .bind(api_key_id)
        .bind(&transcript.id)
        .bind(&version.transcript)
        .bind(format_timestamp(version.created_at)?)
        .execute(&mut *tx)
        .await?;

        for segment in materialization.segments {
            sqlx::query(
                r#"
                INSERT INTO segments (
                    id, api_key_id, transcript_id, position, start_ms, end_ms, text
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&segment.id)
            .bind(api_key_id)
            .bind(&transcript.id)
            .bind(segment.position)
            .bind(segment.start_ms)
            .bind(segment.end_ms)
            .bind(&segment.text)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO embeddings (transcript_id, api_key_id, model, vector, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&transcript.id)
        .bind(api_key_id)
        .bind(&materialization.embedding.model)
        .bind(&materialization.embedding.vector)
        .bind(format_timestamp(materialization.embedding.created_at)?)
        .execute(&mut *tx)
        .await?;

        let result = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET status = 'succeeded', transcript_id = ?, updated_at = ?
            WHERE api_key_id = ?
                AND id = ?
                AND status = 'processing'
                AND transcript_id IS NULL
            "#,
        )
        .bind(&transcript.id)
        .bind(&now)
        .bind(api_key_id)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StorageError::JobNotCompletable {
                job_id: job_id.to_owned(),
            });
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_transcript(
        &self,
        api_key_id: &str,
        transcript_id: &str,
    ) -> Result<Option<TranscriptRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                transcripts.*,
                transcript_versions.id AS current_version_id,
                transcript_versions.transcript AS current_transcript
            FROM transcripts
            JOIN transcript_versions
                ON transcript_versions.transcript_id = transcripts.id
            WHERE transcripts.api_key_id = ?
                AND transcripts.id = ?
                AND transcript_versions.version_number = (
                    SELECT MAX(version_number)
                    FROM transcript_versions
                    WHERE transcript_id = transcripts.id
                )
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(transcript_from_row).transpose()
    }

    pub async fn list_transcripts(
        &self,
        api_key_id: &str,
        filters: &TranscriptFilters,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<TranscriptRecord>, StorageError> {
        self.list_transcripts_in_session(api_key_id, None, filters, cursor, limit)
            .await
    }

    pub async fn list_session_transcripts(
        &self,
        api_key_id: &str,
        session_id: &str,
        filters: &TranscriptFilters,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<TranscriptRecord>, StorageError> {
        self.list_transcripts_in_session(api_key_id, Some(session_id), filters, cursor, limit)
            .await
    }

    async fn list_transcripts_in_session(
        &self,
        api_key_id: &str,
        session_id: Option<&str>,
        filters: &TranscriptFilters,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<TranscriptRecord>, StorageError> {
        let cursor_created_at = cursor
            .as_ref()
            .map(|(created_at, _)| format_timestamp(*created_at))
            .transpose()?;
        let cursor_id = cursor.as_ref().map(|(_, id)| id.as_str());
        let recorded_after = filters.recorded_after.map(format_timestamp).transpose()?;
        let recorded_before = filters.recorded_before.map(format_timestamp).transpose()?;
        let created_after = filters.created_after.map(format_timestamp).transpose()?;
        let created_before = filters.created_before.map(format_timestamp).transpose()?;
        let effective_session_id = session_id.or(filters.session_id.as_deref());
        let mut query = QueryBuilder::new(
            r#"
            SELECT
                transcripts.*,
                transcript_versions.id AS current_version_id,
                transcript_versions.transcript AS current_transcript
            FROM transcripts
            JOIN transcript_versions
                ON transcript_versions.transcript_id = transcripts.id
            WHERE transcripts.api_key_id =
            "#,
        );
        query.push_bind(api_key_id);
        if let Some(session_id) = effective_session_id {
            query.push(" AND transcripts.session_id = ");
            query.push_bind(session_id);
        }
        if let Some(recorded_after) = recorded_after.as_deref() {
            query.push(" AND transcripts.recorded_at > ");
            query.push_bind(recorded_after);
        }
        if let Some(recorded_before) = recorded_before.as_deref() {
            query.push(" AND transcripts.recorded_at <= ");
            query.push_bind(recorded_before);
        }
        if let Some(created_after) = created_after.as_deref() {
            query.push(" AND transcripts.created_at > ");
            query.push_bind(created_after);
        }
        if let Some(created_before) = created_before.as_deref() {
            query.push(" AND transcripts.created_at <= ");
            query.push_bind(created_before);
        }
        for tag_id in &filters.tag_ids {
            query.push(
                r#"
                AND EXISTS (
                    SELECT 1
                    FROM transcript_tags
                    WHERE transcript_tags.api_key_id = transcripts.api_key_id
                        AND transcript_tags.transcript_id = transcripts.id
                        AND transcript_tags.tag_id =
                "#,
            );
            query.push_bind(tag_id);
            query.push(")");
        }
        query.push(
            r#"
                AND transcript_versions.version_number = (
                    SELECT MAX(version_number)
                    FROM transcript_versions
                    WHERE transcript_id = transcripts.id
                )
            "#,
        );
        if let Some(cursor_created_at) = cursor_created_at.as_deref() {
            query.push(
                r#"
                AND (
                    transcripts.created_at <
                "#,
            );
            query.push_bind(cursor_created_at);
            query.push(" OR (transcripts.created_at = ");
            query.push_bind(cursor_created_at);
            query.push(" AND transcripts.id < ");
            query.push_bind(cursor_id.expect("cursor id is present with cursor timestamp"));
            query.push("))");
        }
        query.push(" ORDER BY transcripts.created_at DESC, transcripts.id DESC LIMIT ");
        query.push_bind(limit);
        let rows = query.build().fetch_all(&self.pool).await?;

        rows.into_iter().map(transcript_from_row).collect()
    }

    pub async fn list_transcript_versions(
        &self,
        api_key_id: &str,
        transcript_id: &str,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<TranscriptVersionRecord>, StorageError> {
        let cursor_created_at = cursor
            .as_ref()
            .map(|(created_at, _)| format_timestamp(*created_at))
            .transpose()?;
        let cursor_id = cursor.as_ref().map(|(_, id)| id.as_str());
        let rows = sqlx::query(
            r#"
            SELECT id, transcript_id, transcript, created_at
            FROM transcript_versions
            WHERE api_key_id = ?
                AND transcript_id = ?
                AND (
                    ? IS NULL
                    OR created_at < ?
                    OR (created_at = ? AND id < ?)
                )
            ORDER BY created_at DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(transcript_version_from_row).collect()
    }

    pub async fn list_segments(
        &self,
        api_key_id: &str,
        transcript_id: &str,
    ) -> Result<Vec<SegmentRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, transcript_id, position, start_ms, end_ms, text
            FROM segments
            WHERE api_key_id = ? AND transcript_id = ?
            ORDER BY position ASC
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(segment_from_row).collect()
    }

    pub async fn list_segments_page(
        &self,
        api_key_id: &str,
        transcript_id: &str,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SegmentRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, transcript_id, position, start_ms, end_ms, text
            FROM segments
            WHERE api_key_id = ?
                AND transcript_id = ?
                AND (? IS NULL OR position > ?)
            ORDER BY position ASC
            LIMIT ?
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .bind(cursor)
        .bind(cursor)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(segment_from_row).collect()
    }

    pub async fn session_exists(
        &self,
        api_key_id: &str,
        session_id: &str,
    ) -> Result<bool, StorageError> {
        let exists: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT 1
            FROM sessions
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(exists.is_some())
    }

    pub async fn replace_current_embedding(
        &self,
        api_key_id: &str,
        transcript_id: &str,
        embedding: NewEmbedding,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT INTO embeddings (transcript_id, api_key_id, model, vector, created_at)
            SELECT ?, ?, ?, ?, ?
            WHERE EXISTS (
                SELECT 1 FROM transcripts WHERE api_key_id = ? AND id = ?
            )
            ON CONFLICT(transcript_id) DO UPDATE SET
                model = excluded.model,
                vector = excluded.vector,
                created_at = excluded.created_at
            WHERE embeddings.api_key_id = excluded.api_key_id
            "#,
        )
        .bind(transcript_id)
        .bind(api_key_id)
        .bind(&embedding.model)
        .bind(&embedding.vector)
        .bind(format_timestamp(embedding.created_at)?)
        .bind(api_key_id)
        .bind(transcript_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn get_current_embedding(
        &self,
        api_key_id: &str,
        transcript_id: &str,
    ) -> Result<Option<EmbeddingRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT transcript_id, model, vector, created_at
            FROM embeddings
            WHERE api_key_id = ? AND transcript_id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(embedding_from_row).transpose()
    }

    pub async fn delete_transcript(
        &self,
        api_key_id: &str,
        transcript_id: &str,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            DELETE FROM transcripts
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn create_tag(&self, input: NewTag) -> Result<CreateTagOutcome, StorageError> {
        let mut tx = self.pool.begin().await?;
        let name_folded = fold_tag_name(&input.name);
        let created_at = format_timestamp(input.created_at)?;
        let result = sqlx::query(
            r#"
            INSERT INTO tags (id, api_key_id, name, name_folded, created_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(api_key_id, name_folded) DO NOTHING
            "#,
        )
        .bind(&input.id)
        .bind(&input.api_key_id)
        .bind(&input.name)
        .bind(&name_folded)
        .bind(&created_at)
        .execute(&mut *tx)
        .await?;

        let row = if result.rows_affected() == 1 {
            sqlx::query(
                r#"
                SELECT id, api_key_id, name, created_at
                FROM tags
                WHERE api_key_id = ? AND id = ?
                "#,
            )
            .bind(&input.api_key_id)
            .bind(&input.id)
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, api_key_id, name, created_at
                FROM tags
                WHERE api_key_id = ? AND name_folded = ?
                "#,
            )
            .bind(&input.api_key_id)
            .bind(&name_folded)
            .fetch_one(&mut *tx)
            .await?
        };
        let tag = tag_from_row(row)?;
        tx.commit().await?;

        if result.rows_affected() == 1 {
            Ok(CreateTagOutcome::Created(tag))
        } else {
            Ok(CreateTagOutcome::Existing(tag))
        }
    }

    pub async fn create_session(&self, input: NewSession) -> Result<SessionRecord, StorageError> {
        let created_at = format_timestamp(input.created_at)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO sessions (id, api_key_id, name, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&input.id)
        .bind(&input.api_key_id)
        .bind(&input.name)
        .bind(&created_at)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query(
            r#"
            SELECT id, api_key_id, name, created_at
            FROM sessions
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(&input.api_key_id)
        .bind(&input.id)
        .fetch_one(&mut *tx)
        .await?;
        let session = session_from_row(row)?;
        tx.commit().await?;
        Ok(session)
    }

    pub async fn delete_session(
        &self,
        api_key_id: &str,
        session_id: &str,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            DELETE FROM sessions
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_tag(&self, api_key_id: &str, tag_id: &str) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            DELETE FROM tags
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(tag_id)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_tag(
        &self,
        api_key_id: &str,
        tag_id: &str,
    ) -> Result<Option<TagRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, api_key_id, name, created_at
            FROM tags
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(tag_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(tag_from_row).transpose()
    }

    pub async fn rename_tag(
        &self,
        api_key_id: &str,
        tag_id: &str,
        name: &str,
    ) -> Result<RenameTagOutcome, StorageError> {
        let mut tx = self.pool.begin().await?;
        let name_folded = fold_tag_name(name);
        let update = sqlx::query(
            r#"
            UPDATE tags
            SET name = ?, name_folded = ?
            WHERE api_key_id = ? AND id = ?
                AND NOT EXISTS (
                    SELECT 1
                    FROM tags AS existing
                    WHERE existing.api_key_id = ?
                        AND existing.name_folded = ?
                        AND existing.id <> ?
                )
            RETURNING id, api_key_id, name, created_at
            "#,
        )
        .bind(name)
        .bind(&name_folded)
        .bind(api_key_id)
        .bind(tag_id)
        .bind(api_key_id)
        .bind(&name_folded)
        .bind(tag_id)
        .fetch_optional(&mut *tx)
        .await;

        match update {
            Ok(Some(row)) => {
                let tag = tag_from_row(row)?;
                tx.commit().await?;
                return Ok(RenameTagOutcome::Renamed(tag));
            }
            Ok(None) => {}
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {}
            Err(error) => return Err(error.into()),
        }

        let current: Option<String> = sqlx::query_scalar(
            r#"
            SELECT id
            FROM tags
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(tag_id)
        .fetch_optional(&mut *tx)
        .await?;

        tx.commit().await?;
        if current.is_some() {
            Ok(RenameTagOutcome::Conflict)
        } else {
            Ok(RenameTagOutcome::NotFound)
        }
    }

    pub async fn replace_transcript_tags(
        &self,
        api_key_id: &str,
        transcript_id: &str,
        tag_ids: &[String],
    ) -> Result<ReplaceTranscriptTagsOutcome, StorageError> {
        let mut seen = HashSet::with_capacity(tag_ids.len());
        for tag_id in tag_ids {
            if !seen.insert(tag_id) {
                return Ok(ReplaceTranscriptTagsOutcome::DuplicateTagIds);
            }
        }

        let mut tx = self.pool.begin().await?;
        let transcript_exists: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT 1
            FROM transcripts
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .fetch_optional(&mut *tx)
        .await?;
        if transcript_exists.is_none() {
            tx.commit().await?;
            return Ok(ReplaceTranscriptTagsOutcome::NotFound);
        }

        for tag_id in tag_ids {
            let tag_exists: Option<i64> = sqlx::query_scalar(
                r#"
                SELECT 1
                FROM tags
                WHERE api_key_id = ? AND id = ?
                "#,
            )
            .bind(api_key_id)
            .bind(tag_id)
            .fetch_optional(&mut *tx)
            .await?;
            if tag_exists.is_none() {
                tx.commit().await?;
                return Ok(ReplaceTranscriptTagsOutcome::NotFound);
            }
        }

        sqlx::query(
            r#"
            DELETE FROM transcript_tags
            WHERE api_key_id = ? AND transcript_id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .execute(&mut *tx)
        .await?;

        for tag_id in tag_ids {
            sqlx::query(
                r#"
                INSERT INTO transcript_tags (api_key_id, transcript_id, tag_id)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(api_key_id)
            .bind(transcript_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(ReplaceTranscriptTagsOutcome::Replaced)
    }

    pub async fn list_transcript_tags(
        &self,
        api_key_id: &str,
        transcript_id: &str,
    ) -> Result<Vec<TagRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT tags.id, tags.api_key_id, tags.name, tags.created_at
            FROM tags
            JOIN transcript_tags
                ON transcript_tags.api_key_id = tags.api_key_id
                AND transcript_tags.tag_id = tags.id
            WHERE transcript_tags.api_key_id = ?
                AND transcript_tags.transcript_id = ?
            ORDER BY tags.created_at DESC, tags.id DESC
            "#,
        )
        .bind(api_key_id)
        .bind(transcript_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(tag_from_row).collect()
    }

    pub async fn list_tags_for_transcripts(
        &self,
        api_key_id: &str,
        transcript_ids: &[String],
    ) -> Result<HashMap<String, Vec<TagRecord>>, StorageError> {
        if transcript_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut query = QueryBuilder::new(
            r#"
            SELECT
                transcript_tags.transcript_id AS transcript_id,
                tags.id AS tag_id,
                tags.api_key_id AS api_key_id,
                tags.name AS name,
                tags.created_at AS created_at
            FROM transcript_tags
            JOIN tags
                ON tags.api_key_id = transcript_tags.api_key_id
                AND tags.id = transcript_tags.tag_id
            WHERE transcript_tags.api_key_id =
            "#,
        );
        query.push_bind(api_key_id);
        query.push(" AND transcript_tags.transcript_id IN (");
        let mut separated = query.separated(", ");
        for transcript_id in transcript_ids {
            separated.push_bind(transcript_id);
        }
        separated.push_unseparated(")");
        query.push(
            " ORDER BY transcript_tags.transcript_id ASC, tags.created_at DESC, tags.id DESC",
        );

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut tags_by_transcript = HashMap::new();
        for transcript_id in transcript_ids {
            tags_by_transcript.insert(transcript_id.clone(), Vec::new());
        }
        for row in rows {
            let transcript_id: String = row.try_get("transcript_id")?;
            let tag = tag_from_prefixed_row(row)?;
            tags_by_transcript
                .entry(transcript_id)
                .or_default()
                .push(tag);
        }

        Ok(tags_by_transcript)
    }
}

impl TranscriptionJobRecord {
    fn matches_submission(&self, input: &NewTranscriptionJob) -> bool {
        self.audio_sha256_hex == input.audio_sha256_hex
            && self.recorded_at == input.recorded_at
            && self.session_id == input.session_id
            && self.language == input.language
    }
}

fn validate_database_path(database_path: &Path) -> Result<(), StorageError> {
    if database_path.is_dir() {
        return Err(StorageError::PathIsDirectory(database_path.to_path_buf()));
    }

    let parent = database_path
        .parent()
        .ok_or_else(|| StorageError::MissingParent(database_path.to_path_buf()))?;

    if !parent.exists() {
        return Err(StorageError::MissingParentDirectory(parent.to_path_buf()));
    }

    if !parent.is_dir() {
        return Err(StorageError::ParentNotDirectory(parent.to_path_buf()));
    }

    let probe_path = parent.join(format!(".oracy-db-write-probe-{}", new_id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe_path)
    {
        Ok(_) => {
            std::fs::remove_file(&probe_path).map_err(|source| {
                StorageError::ParentNotWritable {
                    path: parent.to_path_buf(),
                    source,
                }
            })?;
            Ok(())
        }
        Err(source) => Err(StorageError::ParentNotWritable {
            path: parent.to_path_buf(),
            source,
        }),
    }
}

fn new_id() -> String {
    Ulid::new().to_string()
}

async fn ensure_supported_audio_content_hash_algorithm(
    pool: &SqlitePool,
) -> Result<(), StorageError> {
    let unsupported = sqlx::query(
        r#"
        SELECT audio_content_hash_algorithm
        FROM transcription_jobs
        WHERE audio_content_hash_algorithm != ?
        LIMIT 1
        "#,
    )
    .bind(AUDIO_CONTENT_HASH_ALGORITHM_ID)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = unsupported {
        return Err(StorageError::UnsupportedAudioContentHashAlgorithm {
            expected: AUDIO_CONTENT_HASH_ALGORITHM_ID,
            found: row.try_get("audio_content_hash_algorithm")?,
        });
    }

    Ok(())
}

fn fold_tag_name(value: &str) -> String {
    value.chars().nfd().default_case_fold().nfd().collect()
}

fn format_timestamp(value: OffsetDateTime) -> Result<String, StorageError> {
    Ok(value
        .to_offset(time::UtcOffset::UTC)
        .format(&format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z"
        ))?)
}

fn parse_timestamp(value: String) -> Result<OffsetDateTime, StorageError> {
    Ok(OffsetDateTime::parse(&value, &Rfc3339)?)
}

fn job_from_row(row: sqlx::sqlite::SqliteRow) -> Result<TranscriptionJobRecord, StorageError> {
    let retryable_by_client = row
        .try_get::<Option<i64>, _>("retryable_by_client")?
        .map(|value| value != 0);

    Ok(TranscriptionJobRecord {
        id: row.try_get("id")?,
        api_key_id: row.try_get("api_key_id")?,
        idempotency_key: row.try_get("idempotency_key")?,
        audio_sha256_hex: row.try_get("audio_sha256_hex")?,
        audio_content_hash_algorithm: row.try_get("audio_content_hash_algorithm")?,
        recorded_at: parse_timestamp(row.try_get("recorded_at")?)?,
        session_id: row.try_get("session_id")?,
        language: row.try_get("language")?,
        accepted_audio_path: PathBuf::from(row.try_get::<String, _>("accepted_audio_path")?),
        status: row.try_get("status")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        updated_at: parse_timestamp(row.try_get("updated_at")?)?,
        retry_count: row.try_get("retry_count")?,
        max_retries: row.try_get("max_retries")?,
        next_attempt_at: row
            .try_get::<Option<String>, _>("next_attempt_at")?
            .map(parse_timestamp)
            .transpose()?,
        failure_code: row.try_get("failure_code")?,
        failure_message: row.try_get("failure_message")?,
        retryable_by_client,
        transcript_id: row.try_get("transcript_id")?,
    })
}

fn settings_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SettingsRecord, StorageError> {
    Ok(SettingsRecord {
        transcription_model: row.try_get("transcription_model")?,
    })
}

fn transcript_from_row(row: sqlx::sqlite::SqliteRow) -> Result<TranscriptRecord, StorageError> {
    Ok(TranscriptRecord {
        id: row.try_get("id")?,
        api_key_id: row.try_get("api_key_id")?,
        current_version_id: row.try_get("current_version_id")?,
        transcript: row.try_get("current_transcript")?,
        audio_duration_seconds: row.try_get("audio_duration_seconds")?,
        audio_format: row.try_get("audio_format")?,
        audio_size_bytes: row.try_get("audio_size_bytes")?,
        transcript_language: row.try_get("transcript_language")?,
        model: row.try_get("model")?,
        processing_time_ms: row.try_get("processing_time_ms")?,
        cost_cents: row.try_get("cost_cents")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        recorded_at: parse_timestamp(row.try_get("recorded_at")?)?,
        session_id: row.try_get("session_id")?,
    })
}

fn transcript_version_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<TranscriptVersionRecord, StorageError> {
    Ok(TranscriptVersionRecord {
        id: row.try_get("id")?,
        transcript_id: row.try_get("transcript_id")?,
        transcript: row.try_get("transcript")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn segment_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SegmentRecord, StorageError> {
    Ok(SegmentRecord {
        id: row.try_get("id")?,
        transcript_id: row.try_get("transcript_id")?,
        position: row.try_get("position")?,
        start_ms: row.try_get("start_ms")?,
        end_ms: row.try_get("end_ms")?,
        text: row.try_get("text")?,
    })
}

fn embedding_from_row(row: sqlx::sqlite::SqliteRow) -> Result<EmbeddingRecord, StorageError> {
    Ok(EmbeddingRecord {
        transcript_id: row.try_get("transcript_id")?,
        model: row.try_get("model")?,
        vector: row.try_get("vector")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn tag_from_row(row: sqlx::sqlite::SqliteRow) -> Result<TagRecord, StorageError> {
    Ok(TagRecord {
        id: row.try_get("id")?,
        api_key_id: row.try_get("api_key_id")?,
        name: row.try_get("name")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn tag_from_prefixed_row(row: sqlx::sqlite::SqliteRow) -> Result<TagRecord, StorageError> {
    Ok(TagRecord {
        id: row.try_get("tag_id")?,
        api_key_id: row.try_get("api_key_id")?,
        name: row.try_get("name")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn session_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SessionRecord, StorageError> {
    Ok(SessionRecord {
        id: row.try_get("id")?,
        api_key_id: row.try_get("api_key_id")?,
        name: row.try_get("name")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}
