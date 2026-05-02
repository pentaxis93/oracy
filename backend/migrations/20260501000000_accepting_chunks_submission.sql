PRAGMA foreign_keys = OFF;

DROP TRIGGER IF EXISTS transcription_jobs_accepted_tuple_immutable;
DROP TRIGGER IF EXISTS transcription_jobs_session_owner_insert;

CREATE TABLE transcription_jobs_new (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    audio_sha256_hex TEXT NOT NULL DEFAULT '',
    audio_content_hash_algorithm TEXT NOT NULL DEFAULT 'sha256:chunk-sha256-raw-concat:v1',
    recorded_at TEXT NOT NULL,
    session_id TEXT,
    language TEXT,
    accepted_audio_path TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (status IN ('accepting_chunks', 'queued', 'processing', 'retry_waiting', 'succeeded', 'failed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    max_retries INTEGER NOT NULL CHECK (max_retries >= 0),
    next_attempt_at TEXT,
    failure_code TEXT CHECK (
        failure_code IS NULL OR failure_code IN (
            'audio_invalid',
            'engine_timeout',
            'engine_rate_limited',
            'engine_error',
            'storage_error',
            'internal_error',
            'submission_abandoned'
        )
    ),
    failure_message TEXT,
    retryable_by_client INTEGER CHECK (retryable_by_client IS NULL OR retryable_by_client IN (0, 1)),
    voice_note_id TEXT REFERENCES voice_notes(id) ON DELETE SET NULL,
    chunk_count INTEGER NOT NULL DEFAULT 1 CHECK (chunk_count BETWEEN 1 AND 256),
    audio_format TEXT NOT NULL DEFAULT 'wav' CHECK (audio_format IN ('m4a', 'mp3', 'wav', 'webm')),
    transcription_model TEXT NOT NULL DEFAULT 'gpt-4o-mini-transcribe' CHECK (
        transcription_model IN (
            'gpt-4o-mini-transcribe',
            'gpt-4o-transcribe'
        )
    ),
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id),
    UNIQUE (api_key_id, idempotency_key)
);

INSERT INTO transcription_jobs_new (
    id, api_key_id, idempotency_key, audio_sha256_hex,
    audio_content_hash_algorithm, recorded_at, session_id, language,
    accepted_audio_path, status, created_at, updated_at, retry_count,
    max_retries, next_attempt_at, failure_code, failure_message,
    retryable_by_client, voice_note_id
)
SELECT
    id, api_key_id, idempotency_key, audio_sha256_hex,
    audio_content_hash_algorithm, recorded_at, session_id, language,
    accepted_audio_path, status, created_at, updated_at, retry_count,
    max_retries, next_attempt_at, failure_code, failure_message,
    retryable_by_client, voice_note_id
FROM transcription_jobs;

DROP TABLE transcription_jobs;
ALTER TABLE transcription_jobs_new RENAME TO transcription_jobs;

CREATE UNIQUE INDEX transcription_jobs_owner_id_idx
    ON transcription_jobs (api_key_id, id);

CREATE INDEX transcription_jobs_ready_idx
    ON transcription_jobs (status, next_attempt_at, created_at, id);

CREATE TRIGGER transcription_jobs_open_tuple_immutable
BEFORE UPDATE OF api_key_id, idempotency_key, recorded_at, language, chunk_count, audio_format
ON transcription_jobs
BEGIN
    SELECT RAISE(ABORT, 'open submission tuple is immutable');
END;

CREATE TRIGGER transcription_jobs_session_tuple_immutable
BEFORE UPDATE OF session_id
ON transcription_jobs
WHEN NOT (
    OLD.status = 'accepting_chunks'
    AND NEW.status = 'queued'
    AND NEW.session_id IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'open submission tuple is immutable');
END;

CREATE TRIGGER transcription_jobs_accepted_tuple_immutable
BEFORE UPDATE OF audio_sha256_hex, audio_content_hash_algorithm
ON transcription_jobs
WHEN OLD.audio_sha256_hex <> ''
BEGIN
    SELECT RAISE(ABORT, 'accepted submission tuple is immutable');
END;

CREATE TRIGGER transcription_jobs_session_owner_insert
BEFORE INSERT ON transcription_jobs
WHEN NEW.session_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'job session must belong to same owner')
    WHERE NOT EXISTS (
        SELECT 1 FROM sessions
        WHERE api_key_id = NEW.api_key_id AND id = NEW.session_id
    );
END;

CREATE TABLE transcription_job_chunks (
    api_key_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    chunk_sha256_hex TEXT NOT NULL,
    chunk_path TEXT NOT NULL,
    chunk_size_bytes INTEGER NOT NULL CHECK (chunk_size_bytes >= 0),
    accepted_at TEXT NOT NULL,
    PRIMARY KEY (api_key_id, job_id, chunk_index),
    FOREIGN KEY (api_key_id, job_id) REFERENCES transcription_jobs(api_key_id, id) ON DELETE CASCADE
);

CREATE INDEX transcription_job_chunks_order_idx
    ON transcription_job_chunks (api_key_id, job_id, chunk_index);

PRAGMA foreign_keys = ON;
