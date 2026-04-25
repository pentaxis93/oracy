use std::path::{Path, PathBuf};

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use ulid::Ulid;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionConflict {
    pub job_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionJobRecord {
    pub id: String,
    pub api_key_id: String,
    pub idempotency_key: String,
    pub audio_sha256_hex: String,
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
    pub session_id: Option<String>,
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

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
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
                id, api_key_id, idempotency_key, audio_sha256_hex, recorded_at,
                session_id, language, accepted_audio_path, status, created_at,
                updated_at, retry_count, max_retries
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'queued', ?, ?, 0, ?)
            ON CONFLICT(api_key_id, idempotency_key) DO NOTHING
            "#,
        )
        .bind(&id)
        .bind(&input.api_key_id)
        .bind(&input.idempotency_key)
        .bind(&input.audio_sha256_hex)
        .bind(recorded_at)
        .bind(&input.session_id)
        .bind(&input.language)
        .bind(accepted_audio_path)
        .bind(&now)
        .bind(&now)
        .bind(input.max_retries)
        .execute(&mut *tx)
        .await?;

        let row = if result.rows_affected() == 1 {
            sqlx::query(
                r#"
                SELECT * FROM transcription_jobs
                WHERE api_key_id = ? AND id = ?
                "#,
            )
            .bind(&input.api_key_id)
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT * FROM transcription_jobs
                WHERE api_key_id = ? AND idempotency_key = ?
                "#,
            )
            .bind(&input.api_key_id)
            .bind(&input.idempotency_key)
            .fetch_one(&mut *tx)
            .await?
        };
        let job = job_from_row(row)?;
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
        .bind(&transcript.session_id)
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

fn format_timestamp(value: OffsetDateTime) -> Result<String, StorageError> {
    Ok(value.to_offset(time::UtcOffset::UTC).format(&Rfc3339)?)
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
