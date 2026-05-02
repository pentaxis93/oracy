use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use caseless::Caseless;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Column, QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
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
    #[error("job is not eligible for voice note completion: {job_id}")]
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
pub enum OpenJobOutcome {
    Created(TranscriptionJobRecord),
    ReplayedOpen(TranscriptionJobRecord),
    ReplayedFinalized(TranscriptionJobRecord),
    Conflict(SubmissionConflict),
    SessionNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreChunkOutcome {
    Stored,
    Replayed,
    Conflict,
    NotFound,
    NotAcceptingChunks,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalizeJobOutcome {
    Accepted(TranscriptionJobRecord),
    Replayed(TranscriptionJobRecord),
    MissingChunks,
    NotFound,
    NotAcceptingChunks,
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
pub enum ReplaceVoiceNoteTagsOutcome {
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
pub struct NewOpenTranscriptionJob {
    pub api_key_id: String,
    pub idempotency_key: String,
    pub recorded_at: OffsetDateTime,
    pub session_id: Option<String>,
    pub language: Option<String>,
    pub chunk_count: i64,
    pub audio_format: String,
    pub max_retries: i64,
    pub now: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct AcceptedChunk {
    pub api_key_id: String,
    pub job_id: String,
    pub chunk_index: i64,
    pub chunk_sha256_hex: String,
    pub chunk_path: PathBuf,
    pub chunk_size_bytes: i64,
    pub accepted_at: OffsetDateTime,
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
    pub voice_note_id: Option<String>,
    pub chunk_count: i64,
    pub audio_format: String,
    pub transcription_model: String,
    pub chunks_received: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRecord {
    pub chunk_index: i64,
    pub chunk_sha256_hex: String,
    pub chunk_path: PathBuf,
    pub chunk_size_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct VoiceNoteMaterialization {
    pub voice_note: NewVoiceNote,
    pub initial_version: NewVoiceNoteVersion,
    pub segments: Vec<NewSegment>,
    pub embedding: NewEmbedding,
}

#[derive(Debug, Clone)]
pub struct NewVoiceNote {
    pub id: String,
    pub audio_duration_seconds: f64,
    pub audio_format: String,
    pub audio_size_bytes: i64,
    pub language: Option<String>,
    pub model: String,
    pub processing_time_ms: i64,
    pub cost_cents: Option<i64>,
    pub created_at: OffsetDateTime,
    pub recorded_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct NewVoiceNoteVersion {
    pub id: String,
    pub text: String,
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
pub struct VoiceNoteRecord {
    pub id: String,
    pub api_key_id: String,
    pub current_version_id: String,
    pub text: String,
    pub audio_duration_seconds: f64,
    pub audio_format: String,
    pub audio_size_bytes: i64,
    pub language: Option<String>,
    pub model: String,
    pub processing_time_ms: i64,
    pub cost_cents: Option<i64>,
    pub created_at: OffsetDateTime,
    pub recorded_at: OffsetDateTime,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceNoteVersionRecord {
    pub id: String,
    pub voice_note_id: String,
    pub text: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceNoteFilters {
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
    pub voice_note_id: String,
    pub position: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingRecord {
    pub voice_note_id: String,
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

    async fn begin_immediate_tx(&self) -> Result<Transaction<'static, Sqlite>, StorageError> {
        Ok(self.pool.begin_with("BEGIN IMMEDIATE").await?)
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

    pub async fn open_job(
        &self,
        input: NewOpenTranscriptionJob,
    ) -> Result<OpenJobOutcome, StorageError> {
        let mut tx = self.pool.begin().await?;
        let id = new_id();
        let now = format_timestamp(input.now)?;
        let recorded_at = format_timestamp(input.recorded_at)?;
        sqlx::query(
            r#"
            INSERT INTO transcription_jobs (
                id, api_key_id, idempotency_key, recorded_at, session_id,
                language, status, created_at, updated_at, retry_count,
                max_retries, chunk_count, audio_format
            )
            SELECT ?, ?, ?, ?, ?, ?, 'accepting_chunks', ?, ?, 0, ?, ?, ?
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
        .bind(recorded_at)
        .bind(&input.session_id)
        .bind(&input.language)
        .bind(&now)
        .bind(&now)
        .bind(input.max_retries)
        .bind(input.chunk_count)
        .bind(&input.audio_format)
        .bind(&input.session_id)
        .bind(&input.api_key_id)
        .bind(&input.session_id)
        .execute(&mut *tx)
        .await?;

        let Some(job) =
            select_job_by_idempotency_key(&mut tx, &input.api_key_id, &input.idempotency_key)
                .await?
        else {
            tx.commit().await?;
            return Ok(OpenJobOutcome::SessionNotFound);
        };
        tx.commit().await?;

        if !job.matches_open_submission(&input) {
            return Ok(OpenJobOutcome::Conflict(SubmissionConflict {
                job_id: job.id,
            }));
        }
        if job.id == id {
            return Ok(OpenJobOutcome::Created(job));
        }
        if job.status == "accepting_chunks" {
            return Ok(OpenJobOutcome::ReplayedOpen(job));
        }
        Ok(OpenJobOutcome::ReplayedFinalized(job))
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
        let mut tx = self.pool.begin().await?;
        let job = select_job_by_idempotency_key(&mut tx, api_key_id, idempotency_key).await?;
        tx.commit().await?;
        Ok(job)
    }

    pub async fn get_job(
        &self,
        api_key_id: &str,
        job_id: &str,
    ) -> Result<Option<TranscriptionJobRecord>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let job = select_job_by_id(&mut tx, api_key_id, job_id).await?;
        tx.commit().await?;
        Ok(job)
    }

    pub async fn get_chunk(
        &self,
        api_key_id: &str,
        job_id: &str,
        chunk_index: i64,
    ) -> Result<Option<ChunkRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT chunk_index, chunk_sha256_hex, chunk_path, chunk_size_bytes
            FROM transcription_job_chunks
            WHERE api_key_id = ? AND job_id = ? AND chunk_index = ?
            "#,
        )
        .bind(api_key_id)
        .bind(job_id)
        .bind(chunk_index)
        .fetch_optional(&self.pool)
        .await?;

        row.map(chunk_from_row).transpose()
    }

    pub async fn store_chunk(
        &self,
        chunk: AcceptedChunk,
    ) -> Result<StoreChunkOutcome, StorageError> {
        let mut tx = self.begin_immediate_tx().await?;
        let Some(job) = select_job_by_id(&mut tx, &chunk.api_key_id, &chunk.job_id).await? else {
            tx.commit().await?;
            return Ok(StoreChunkOutcome::NotFound);
        };
        if job.status != "accepting_chunks" {
            tx.commit().await?;
            return Ok(StoreChunkOutcome::NotAcceptingChunks);
        }

        let accepted_at = format_timestamp(chunk.accepted_at)?;
        let chunk_path = chunk.chunk_path.to_string_lossy().into_owned();
        let result = sqlx::query(
            r#"
            INSERT INTO transcription_job_chunks (
                api_key_id, job_id, chunk_index, chunk_sha256_hex,
                chunk_path, chunk_size_bytes, accepted_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(api_key_id, job_id, chunk_index) DO NOTHING
            "#,
        )
        .bind(&chunk.api_key_id)
        .bind(&chunk.job_id)
        .bind(chunk.chunk_index)
        .bind(&chunk.chunk_sha256_hex)
        .bind(&chunk_path)
        .bind(chunk.chunk_size_bytes)
        .bind(&accepted_at)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 1 {
            sqlx::query(
                r#"
                UPDATE transcription_jobs
                SET updated_at = ?
                WHERE api_key_id = ? AND id = ?
                "#,
            )
            .bind(&accepted_at)
            .bind(&chunk.api_key_id)
            .bind(&chunk.job_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(StoreChunkOutcome::Stored);
        }

        let existing =
            select_chunk_by_index(&mut tx, &chunk.api_key_id, &chunk.job_id, chunk.chunk_index)
                .await?
                .expect("conflicting chunk row exists");
        tx.commit().await?;
        if existing.chunk_sha256_hex == chunk.chunk_sha256_hex {
            Ok(StoreChunkOutcome::Replayed)
        } else {
            Ok(StoreChunkOutcome::Conflict)
        }
    }

    pub async fn list_chunks_for_finalize(
        &self,
        api_key_id: &str,
        job_id: &str,
    ) -> Result<Option<(TranscriptionJobRecord, Vec<ChunkRecord>)>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let Some(job) = select_job_by_id(&mut tx, api_key_id, job_id).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        let rows = sqlx::query(
            r#"
            SELECT chunk_index, chunk_sha256_hex, chunk_path, chunk_size_bytes
            FROM transcription_job_chunks
            WHERE api_key_id = ? AND job_id = ?
            ORDER BY chunk_index ASC
            "#,
        )
        .bind(api_key_id)
        .bind(job_id)
        .fetch_all(&mut *tx)
        .await?;
        let chunks = rows
            .into_iter()
            .map(chunk_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        tx.commit().await?;
        Ok(Some((job, chunks)))
    }

    pub async fn finalize_job(
        &self,
        api_key_id: &str,
        job_id: &str,
        audio_sha256_hex: &str,
        accepted_audio_path: &Path,
        transcription_model: &str,
        now: OffsetDateTime,
    ) -> Result<FinalizeJobOutcome, StorageError> {
        let mut tx = self.begin_immediate_tx().await?;
        let Some(job) = select_job_by_id(&mut tx, api_key_id, job_id).await? else {
            tx.commit().await?;
            return Ok(FinalizeJobOutcome::NotFound);
        };
        if job.status != "accepting_chunks" {
            tx.commit().await?;
            if job.audio_sha256_hex == audio_sha256_hex && !job.audio_sha256_hex.is_empty() {
                return Ok(FinalizeJobOutcome::Replayed(job));
            }
            return Ok(FinalizeJobOutcome::NotAcceptingChunks);
        }

        let accepted_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM transcription_job_chunks
            WHERE api_key_id = ? AND job_id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(job_id)
        .fetch_one(&mut *tx)
        .await?;
        if accepted_count != job.chunk_count {
            tx.commit().await?;
            return Ok(FinalizeJobOutcome::MissingChunks);
        }

        let now = format_timestamp(now)?;
        let accepted_audio_path = accepted_audio_path.to_string_lossy().into_owned();
        sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET audio_sha256_hex = ?,
                accepted_audio_path = ?,
                transcription_model = ?,
                status = 'queued',
                updated_at = ?
            WHERE api_key_id = ? AND id = ? AND status = 'accepting_chunks'
            "#,
        )
        .bind(audio_sha256_hex)
        .bind(accepted_audio_path)
        .bind(transcription_model)
        .bind(&now)
        .bind(api_key_id)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        let job = select_job_by_id(&mut tx, api_key_id, job_id)
            .await?
            .expect("finalized job is visible");
        tx.commit().await?;
        Ok(FinalizeJobOutcome::Accepted(job))
    }

    pub async fn complete_job_with_voice_note(
        &self,
        api_key_id: &str,
        job_id: &str,
        materialization: VoiceNoteMaterialization,
    ) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let voice_note = materialization.voice_note;
        let version = materialization.initial_version;
        let now = format_timestamp(voice_note.created_at)?;

        let result = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET updated_at = ?
            WHERE api_key_id = ?
                AND id = ?
                AND status = 'processing'
                AND voice_note_id IS NULL
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

        let voice_note_session_id: Option<String> = sqlx::query(
            r#"
            SELECT sessions.id AS voice_note_session_id
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
        .try_get("voice_note_session_id")?;

        sqlx::query(
            r#"
            INSERT INTO voice_notes (
                id, api_key_id, audio_duration_seconds, audio_format, audio_size_bytes,
                language, model, processing_time_ms, cost_cents,
                created_at, recorded_at, session_id
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&voice_note.id)
        .bind(api_key_id)
        .bind(voice_note.audio_duration_seconds)
        .bind(&voice_note.audio_format)
        .bind(voice_note.audio_size_bytes)
        .bind(&voice_note.language)
        .bind(&voice_note.model)
        .bind(voice_note.processing_time_ms)
        .bind(voice_note.cost_cents)
        .bind(&now)
        .bind(format_timestamp(voice_note.recorded_at)?)
        .bind(&voice_note_session_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO voice_note_versions (
                id, api_key_id, voice_note_id, version_number, text, created_at
            )
            VALUES (?, ?, ?, 1, ?, ?)
            "#,
        )
        .bind(&version.id)
        .bind(api_key_id)
        .bind(&voice_note.id)
        .bind(&version.text)
        .bind(format_timestamp(version.created_at)?)
        .execute(&mut *tx)
        .await?;

        for segment in materialization.segments {
            sqlx::query(
                r#"
                INSERT INTO segments (
                    id, api_key_id, voice_note_id, position, start_ms, end_ms, text
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&segment.id)
            .bind(api_key_id)
            .bind(&voice_note.id)
            .bind(segment.position)
            .bind(segment.start_ms)
            .bind(segment.end_ms)
            .bind(&segment.text)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&voice_note.id)
        .bind(api_key_id)
        .bind(&materialization.embedding.model)
        .bind(&materialization.embedding.vector)
        .bind(format_timestamp(materialization.embedding.created_at)?)
        .execute(&mut *tx)
        .await?;

        let result = sqlx::query(
            r#"
            UPDATE transcription_jobs
            SET status = 'succeeded', voice_note_id = ?, updated_at = ?
            WHERE api_key_id = ?
                AND id = ?
                AND status = 'processing'
                AND voice_note_id IS NULL
            "#,
        )
        .bind(&voice_note.id)
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

    pub async fn get_voice_note(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
    ) -> Result<Option<VoiceNoteRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT
                voice_notes.*,
                voice_note_versions.id AS current_version_id,
                voice_note_versions.text AS current_text
            FROM voice_notes
            JOIN voice_note_versions
                ON voice_note_versions.voice_note_id = voice_notes.id
            WHERE voice_notes.api_key_id = ?
                AND voice_notes.id = ?
                AND voice_note_versions.version_number = (
                    SELECT MAX(version_number)
                    FROM voice_note_versions
                    WHERE voice_note_id = voice_notes.id
                )
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(voice_note_from_row).transpose()
    }

    pub async fn list_voice_notes(
        &self,
        api_key_id: &str,
        filters: &VoiceNoteFilters,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<VoiceNoteRecord>, StorageError> {
        self.list_voice_notes_in_session(api_key_id, None, filters, cursor, limit)
            .await
    }

    pub async fn list_session_voice_notes(
        &self,
        api_key_id: &str,
        session_id: &str,
        filters: &VoiceNoteFilters,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<VoiceNoteRecord>, StorageError> {
        self.list_voice_notes_in_session(api_key_id, Some(session_id), filters, cursor, limit)
            .await
    }

    async fn list_voice_notes_in_session(
        &self,
        api_key_id: &str,
        session_id: Option<&str>,
        filters: &VoiceNoteFilters,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<VoiceNoteRecord>, StorageError> {
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
                voice_notes.*,
                voice_note_versions.id AS current_version_id,
                voice_note_versions.text AS current_text
            FROM voice_notes
            JOIN voice_note_versions
                ON voice_note_versions.voice_note_id = voice_notes.id
            WHERE voice_notes.api_key_id =
            "#,
        );
        query.push_bind(api_key_id);
        if let Some(session_id) = effective_session_id {
            query.push(" AND voice_notes.session_id = ");
            query.push_bind(session_id);
        }
        if let Some(recorded_after) = recorded_after.as_deref() {
            query.push(" AND voice_notes.recorded_at > ");
            query.push_bind(recorded_after);
        }
        if let Some(recorded_before) = recorded_before.as_deref() {
            query.push(" AND voice_notes.recorded_at <= ");
            query.push_bind(recorded_before);
        }
        if let Some(created_after) = created_after.as_deref() {
            query.push(" AND voice_notes.created_at > ");
            query.push_bind(created_after);
        }
        if let Some(created_before) = created_before.as_deref() {
            query.push(" AND voice_notes.created_at <= ");
            query.push_bind(created_before);
        }
        for tag_id in &filters.tag_ids {
            query.push(
                r#"
                AND EXISTS (
                    SELECT 1
                    FROM voice_note_tags
                    WHERE voice_note_tags.api_key_id = voice_notes.api_key_id
                        AND voice_note_tags.voice_note_id = voice_notes.id
                        AND voice_note_tags.tag_id =
                "#,
            );
            query.push_bind(tag_id);
            query.push(")");
        }
        query.push(
            r#"
                AND voice_note_versions.version_number = (
                    SELECT MAX(version_number)
                    FROM voice_note_versions
                    WHERE voice_note_id = voice_notes.id
                )
            "#,
        );
        if let Some(cursor_created_at) = cursor_created_at.as_deref() {
            query.push(
                r#"
                AND (
                    voice_notes.created_at <
                "#,
            );
            query.push_bind(cursor_created_at);
            query.push(" OR (voice_notes.created_at = ");
            query.push_bind(cursor_created_at);
            query.push(" AND voice_notes.id < ");
            query.push_bind(cursor_id.expect("cursor id is present with cursor timestamp"));
            query.push("))");
        }
        query.push(" ORDER BY voice_notes.created_at DESC, voice_notes.id DESC LIMIT ");
        query.push_bind(limit);
        let rows = query.build().fetch_all(&self.pool).await?;

        rows.into_iter().map(voice_note_from_row).collect()
    }

    pub async fn list_voice_note_versions(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<VoiceNoteVersionRecord>, StorageError> {
        let cursor_created_at = cursor
            .as_ref()
            .map(|(created_at, _)| format_timestamp(*created_at))
            .transpose()?;
        let cursor_id = cursor.as_ref().map(|(_, id)| id.as_str());
        let rows = sqlx::query(
            r#"
            SELECT id, voice_note_id, text, created_at
            FROM voice_note_versions
            WHERE api_key_id = ?
                AND voice_note_id = ?
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
        .bind(voice_note_id)
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(voice_note_version_from_row).collect()
    }

    pub async fn list_segments(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
    ) -> Result<Vec<SegmentRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, voice_note_id, position, start_ms, end_ms, text
            FROM segments
            WHERE api_key_id = ? AND voice_note_id = ?
            ORDER BY position ASC
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(segment_from_row).collect()
    }

    pub async fn list_segments_page(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<SegmentRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT id, voice_note_id, position, start_ms, end_ms, text
            FROM segments
            WHERE api_key_id = ?
                AND voice_note_id = ?
                AND (? IS NULL OR position > ?)
            ORDER BY position ASC
            LIMIT ?
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
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

    pub async fn list_tags(
        &self,
        api_key_id: &str,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<TagRecord>, StorageError> {
        let cursor_created_at = cursor
            .as_ref()
            .map(|(created_at, _)| format_timestamp(*created_at))
            .transpose()?;
        let cursor_id = cursor.as_ref().map(|(_, id)| id.as_str());
        let rows = sqlx::query(
            r#"
            SELECT id, api_key_id, name, created_at
            FROM tags
            WHERE api_key_id = ?
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
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(tag_from_row).collect()
    }

    pub async fn list_sessions(
        &self,
        api_key_id: &str,
        cursor: Option<(OffsetDateTime, String)>,
        limit: i64,
    ) -> Result<Vec<SessionRecord>, StorageError> {
        let cursor_created_at = cursor
            .as_ref()
            .map(|(created_at, _)| format_timestamp(*created_at))
            .transpose()?;
        let cursor_id = cursor.as_ref().map(|(_, id)| id.as_str());
        let rows = sqlx::query(
            r#"
            SELECT id, api_key_id, name, created_at
            FROM sessions
            WHERE api_key_id = ?
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
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(&cursor_created_at)
        .bind(cursor_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(session_from_row).collect()
    }

    pub async fn replace_current_embedding(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
        embedding: NewEmbedding,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            INSERT INTO embeddings (voice_note_id, api_key_id, model, vector, created_at)
            SELECT ?, ?, ?, ?, ?
            WHERE EXISTS (
                SELECT 1 FROM voice_notes WHERE api_key_id = ? AND id = ?
            )
            ON CONFLICT(voice_note_id) DO UPDATE SET
                model = excluded.model,
                vector = excluded.vector,
                created_at = excluded.created_at
            WHERE embeddings.api_key_id = excluded.api_key_id
            "#,
        )
        .bind(voice_note_id)
        .bind(api_key_id)
        .bind(&embedding.model)
        .bind(&embedding.vector)
        .bind(format_timestamp(embedding.created_at)?)
        .bind(api_key_id)
        .bind(voice_note_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn get_current_embedding(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
    ) -> Result<Option<EmbeddingRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT voice_note_id, model, vector, created_at
            FROM embeddings
            WHERE api_key_id = ? AND voice_note_id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(embedding_from_row).transpose()
    }

    pub async fn delete_voice_note(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            DELETE FROM voice_notes
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
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
                UPDATE tags
                SET name = ?
                WHERE api_key_id = ? AND name_folded = ?
                RETURNING id, api_key_id, name, created_at
                "#,
            )
            .bind(&input.name)
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

    pub async fn get_session(
        &self,
        api_key_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            SELECT id, api_key_id, name, created_at
            FROM sessions
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(session_from_row).transpose()
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

    pub async fn rename_session(
        &self,
        api_key_id: &str,
        session_id: &str,
        name: &str,
    ) -> Result<Option<SessionRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            UPDATE sessions
            SET name = ?
            WHERE api_key_id = ? AND id = ?
            RETURNING id, api_key_id, name, created_at
            "#,
        )
        .bind(name)
        .bind(api_key_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(session_from_row).transpose()
    }

    pub async fn replace_voice_note_tags(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
        tag_ids: &[String],
    ) -> Result<ReplaceVoiceNoteTagsOutcome, StorageError> {
        let mut seen = HashSet::with_capacity(tag_ids.len());
        for tag_id in tag_ids {
            if !seen.insert(tag_id) {
                return Ok(ReplaceVoiceNoteTagsOutcome::DuplicateTagIds);
            }
        }

        let mut tx = self.pool.begin().await?;
        let voice_note_exists: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT 1
            FROM voice_notes
            WHERE api_key_id = ? AND id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
        .fetch_optional(&mut *tx)
        .await?;
        if voice_note_exists.is_none() {
            tx.commit().await?;
            return Ok(ReplaceVoiceNoteTagsOutcome::NotFound);
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
                return Ok(ReplaceVoiceNoteTagsOutcome::NotFound);
            }
        }

        sqlx::query(
            r#"
            DELETE FROM voice_note_tags
            WHERE api_key_id = ? AND voice_note_id = ?
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
        .execute(&mut *tx)
        .await?;

        for tag_id in tag_ids {
            sqlx::query(
                r#"
                INSERT INTO voice_note_tags (api_key_id, voice_note_id, tag_id)
                VALUES (?, ?, ?)
                "#,
            )
            .bind(api_key_id)
            .bind(voice_note_id)
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(ReplaceVoiceNoteTagsOutcome::Replaced)
    }

    pub async fn list_voice_note_tags(
        &self,
        api_key_id: &str,
        voice_note_id: &str,
    ) -> Result<Vec<TagRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            SELECT tags.id, tags.api_key_id, tags.name, tags.created_at
            FROM tags
            JOIN voice_note_tags
                ON voice_note_tags.api_key_id = tags.api_key_id
                AND voice_note_tags.tag_id = tags.id
            WHERE voice_note_tags.api_key_id = ?
                AND voice_note_tags.voice_note_id = ?
            ORDER BY tags.created_at DESC, tags.id DESC
            "#,
        )
        .bind(api_key_id)
        .bind(voice_note_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(tag_from_row).collect()
    }

    pub async fn list_tags_for_voice_notes(
        &self,
        api_key_id: &str,
        voice_note_ids: &[String],
    ) -> Result<HashMap<String, Vec<TagRecord>>, StorageError> {
        if voice_note_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut query = QueryBuilder::new(
            r#"
            SELECT
                voice_note_tags.voice_note_id AS voice_note_id,
                tags.id AS tag_id,
                tags.api_key_id AS api_key_id,
                tags.name AS name,
                tags.created_at AS created_at
            FROM voice_note_tags
            JOIN tags
                ON tags.api_key_id = voice_note_tags.api_key_id
                AND tags.id = voice_note_tags.tag_id
            WHERE voice_note_tags.api_key_id =
            "#,
        );
        query.push_bind(api_key_id);
        query.push(" AND voice_note_tags.voice_note_id IN (");
        let mut separated = query.separated(", ");
        for voice_note_id in voice_note_ids {
            separated.push_bind(voice_note_id);
        }
        separated.push_unseparated(")");
        query.push(
            " ORDER BY voice_note_tags.voice_note_id ASC, tags.created_at DESC, tags.id DESC",
        );

        let rows = query.build().fetch_all(&self.pool).await?;
        let mut tags_by_voice_note = HashMap::new();
        for voice_note_id in voice_note_ids {
            tags_by_voice_note.insert(voice_note_id.clone(), Vec::new());
        }
        for row in rows {
            let voice_note_id: String = row.try_get("voice_note_id")?;
            let tag = tag_from_prefixed_row(row)?;
            tags_by_voice_note
                .entry(voice_note_id)
                .or_default()
                .push(tag);
        }

        Ok(tags_by_voice_note)
    }
}

impl TranscriptionJobRecord {
    fn matches_submission(&self, input: &NewTranscriptionJob) -> bool {
        self.audio_sha256_hex == input.audio_sha256_hex
            && self.recorded_at == input.recorded_at
            && self.session_id == input.session_id
            && self.language == input.language
    }

    fn matches_open_submission(&self, input: &NewOpenTranscriptionJob) -> bool {
        self.recorded_at == input.recorded_at
            && self.session_id == input.session_id
            && self.language == input.language
            && self.chunk_count == input.chunk_count
            && self.audio_format == input.audio_format
    }
}

async fn select_job_by_id(
    tx: &mut Transaction<'_, Sqlite>,
    api_key_id: &str,
    job_id: &str,
) -> Result<Option<TranscriptionJobRecord>, StorageError> {
    let row = sqlx::query(
        r#"
        SELECT
            transcription_jobs.*,
            COUNT(transcription_job_chunks.chunk_index) AS chunks_received
        FROM transcription_jobs
        LEFT JOIN transcription_job_chunks
            ON transcription_job_chunks.api_key_id = transcription_jobs.api_key_id
            AND transcription_job_chunks.job_id = transcription_jobs.id
        WHERE transcription_jobs.api_key_id = ? AND transcription_jobs.id = ?
        GROUP BY transcription_jobs.id
        "#,
    )
    .bind(api_key_id)
    .bind(job_id)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(job_from_row).transpose()
}

async fn select_job_by_idempotency_key(
    tx: &mut Transaction<'_, Sqlite>,
    api_key_id: &str,
    idempotency_key: &str,
) -> Result<Option<TranscriptionJobRecord>, StorageError> {
    let row = sqlx::query(
        r#"
        SELECT
            transcription_jobs.*,
            COUNT(transcription_job_chunks.chunk_index) AS chunks_received
        FROM transcription_jobs
        LEFT JOIN transcription_job_chunks
            ON transcription_job_chunks.api_key_id = transcription_jobs.api_key_id
            AND transcription_job_chunks.job_id = transcription_jobs.id
        WHERE transcription_jobs.api_key_id = ?
            AND transcription_jobs.idempotency_key = ?
        GROUP BY transcription_jobs.id
        "#,
    )
    .bind(api_key_id)
    .bind(idempotency_key)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(job_from_row).transpose()
}

async fn select_chunk_by_index(
    tx: &mut Transaction<'_, Sqlite>,
    api_key_id: &str,
    job_id: &str,
    chunk_index: i64,
) -> Result<Option<ChunkRecord>, StorageError> {
    let row = sqlx::query(
        r#"
        SELECT chunk_index, chunk_sha256_hex, chunk_path, chunk_size_bytes
        FROM transcription_job_chunks
        WHERE api_key_id = ? AND job_id = ? AND chunk_index = ?
        "#,
    )
    .bind(api_key_id)
    .bind(job_id)
    .bind(chunk_index)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(chunk_from_row).transpose()
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
        voice_note_id: row.try_get("voice_note_id")?,
        chunk_count: row.try_get("chunk_count")?,
        audio_format: row.try_get("audio_format")?,
        transcription_model: row.try_get("transcription_model")?,
        chunks_received: optional_i64(&row, "chunks_received")?.unwrap_or(0),
    })
}

fn chunk_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ChunkRecord, StorageError> {
    Ok(ChunkRecord {
        chunk_index: row.try_get("chunk_index")?,
        chunk_sha256_hex: row.try_get("chunk_sha256_hex")?,
        chunk_path: PathBuf::from(row.try_get::<String, _>("chunk_path")?),
        chunk_size_bytes: row.try_get("chunk_size_bytes")?,
    })
}

fn optional_i64(
    row: &sqlx::sqlite::SqliteRow,
    column_name: &str,
) -> Result<Option<i64>, StorageError> {
    if row
        .columns()
        .iter()
        .any(|column| column.name() == column_name)
    {
        Ok(Some(row.try_get(column_name)?))
    } else {
        Ok(None)
    }
}

fn settings_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SettingsRecord, StorageError> {
    Ok(SettingsRecord {
        transcription_model: row.try_get("transcription_model")?,
    })
}

fn voice_note_from_row(row: sqlx::sqlite::SqliteRow) -> Result<VoiceNoteRecord, StorageError> {
    Ok(VoiceNoteRecord {
        id: row.try_get("id")?,
        api_key_id: row.try_get("api_key_id")?,
        current_version_id: row.try_get("current_version_id")?,
        text: row.try_get("current_text")?,
        audio_duration_seconds: row.try_get("audio_duration_seconds")?,
        audio_format: row.try_get("audio_format")?,
        audio_size_bytes: row.try_get("audio_size_bytes")?,
        language: row.try_get("language")?,
        model: row.try_get("model")?,
        processing_time_ms: row.try_get("processing_time_ms")?,
        cost_cents: row.try_get("cost_cents")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
        recorded_at: parse_timestamp(row.try_get("recorded_at")?)?,
        session_id: row.try_get("session_id")?,
    })
}

fn voice_note_version_from_row(
    row: sqlx::sqlite::SqliteRow,
) -> Result<VoiceNoteVersionRecord, StorageError> {
    Ok(VoiceNoteVersionRecord {
        id: row.try_get("id")?,
        voice_note_id: row.try_get("voice_note_id")?,
        text: row.try_get("text")?,
        created_at: parse_timestamp(row.try_get("created_at")?)?,
    })
}

fn segment_from_row(row: sqlx::sqlite::SqliteRow) -> Result<SegmentRecord, StorageError> {
    Ok(SegmentRecord {
        id: row.try_get("id")?,
        voice_note_id: row.try_get("voice_note_id")?,
        position: row.try_get("position")?,
        start_ms: row.try_get("start_ms")?,
        end_ms: row.try_get("end_ms")?,
        text: row.try_get("text")?,
    })
}

fn embedding_from_row(row: sqlx::sqlite::SqliteRow) -> Result<EmbeddingRecord, StorageError> {
    Ok(EmbeddingRecord {
        voice_note_id: row.try_get("voice_note_id")?,
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
