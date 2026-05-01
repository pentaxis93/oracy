PRAGMA foreign_keys = ON;

CREATE TABLE transcription_jobs (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    audio_sha256_hex TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    session_id TEXT,
    language TEXT,
    accepted_audio_path TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'processing', 'retry_waiting', 'succeeded', 'failed')),
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
            'internal_error'
        )
    ),
    failure_message TEXT,
    retryable_by_client INTEGER CHECK (retryable_by_client IS NULL OR retryable_by_client IN (0, 1)),
    voice_note_id TEXT REFERENCES voice_notes(id) ON DELETE SET NULL,
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id),
    UNIQUE (api_key_id, idempotency_key)
);

CREATE INDEX transcription_jobs_owner_id_idx
    ON transcription_jobs (api_key_id, id);

CREATE INDEX transcription_jobs_ready_idx
    ON transcription_jobs (status, next_attempt_at, created_at, id);

CREATE TRIGGER transcription_jobs_accepted_tuple_immutable
BEFORE UPDATE OF api_key_id, idempotency_key, audio_sha256_hex, recorded_at, session_id, language
ON transcription_jobs
BEGIN
    SELECT RAISE(ABORT, 'accepted submission tuple is immutable');
END;

CREATE TABLE voice_notes (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    audio_duration_seconds REAL NOT NULL CHECK (audio_duration_seconds >= 0),
    audio_format TEXT NOT NULL,
    audio_size_bytes INTEGER NOT NULL CHECK (audio_size_bytes >= 0),
    language TEXT,
    model TEXT NOT NULL,
    processing_time_ms INTEGER NOT NULL CHECK (processing_time_ms >= 0),
    cost_cents INTEGER CHECK (cost_cents IS NULL OR cost_cents >= 0),
    created_at TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    session_id TEXT
);

CREATE UNIQUE INDEX voice_notes_owner_id_idx
    ON voice_notes (api_key_id, id);

CREATE INDEX voice_notes_history_idx
    ON voice_notes (api_key_id, created_at DESC, id DESC);

CREATE TABLE voice_note_versions (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    voice_note_id TEXT NOT NULL,
    version_number INTEGER NOT NULL CHECK (version_number >= 1),
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id) ON DELETE CASCADE,
    UNIQUE (voice_note_id, version_number)
);

CREATE INDEX voice_note_versions_current_idx
    ON voice_note_versions (voice_note_id, version_number DESC);

CREATE TABLE segments (
    id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    voice_note_id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms >= start_ms),
    text TEXT NOT NULL,
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id) ON DELETE CASCADE,
    UNIQUE (voice_note_id, position)
);

CREATE INDEX segments_order_idx
    ON segments (voice_note_id, position ASC);

CREATE TABLE embeddings (
    voice_note_id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    model TEXT NOT NULL,
    vector BLOB NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id) ON DELETE CASCADE
);

CREATE INDEX embeddings_owner_idx
    ON embeddings (api_key_id, voice_note_id);
