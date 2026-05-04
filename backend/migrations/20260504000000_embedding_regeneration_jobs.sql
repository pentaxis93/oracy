PRAGMA foreign_keys = ON;

CREATE TABLE embedding_regeneration_jobs (
    voice_note_id TEXT PRIMARY KEY,
    api_key_id TEXT NOT NULL,
    voice_note_version_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'processing', 'retry_waiting', 'failed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    max_retries INTEGER NOT NULL CHECK (max_retries >= 0),
    next_attempt_at TEXT,
    failure_code TEXT CHECK (
        failure_code IS NULL OR failure_code IN (
            'engine_timeout',
            'engine_rate_limited',
            'engine_error',
            'storage_error',
            'internal_error'
        )
    ),
    failure_message TEXT,
    processing_lease_token TEXT,
    processing_lease_expires_at TEXT,
    FOREIGN KEY (api_key_id, voice_note_id) REFERENCES voice_notes(api_key_id, id) ON DELETE CASCADE,
    FOREIGN KEY (voice_note_version_id) REFERENCES voice_note_versions(id) ON DELETE CASCADE
);

CREATE INDEX embedding_regeneration_jobs_ready_idx
    ON embedding_regeneration_jobs (status, next_attempt_at, created_at, voice_note_id);

CREATE INDEX embedding_regeneration_jobs_processing_lease_idx
    ON embedding_regeneration_jobs (status, processing_lease_expires_at, next_attempt_at, created_at, voice_note_id);

INSERT INTO embedding_regeneration_jobs (
    voice_note_id, api_key_id, voice_note_version_id, status, created_at,
    updated_at, retry_count, max_retries, next_attempt_at, failure_code,
    failure_message, processing_lease_token, processing_lease_expires_at
)
SELECT
    voice_notes.id,
    voice_notes.api_key_id,
    current_versions.id,
    'queued',
    current_versions.created_at,
    current_versions.created_at,
    0,
    3,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL
FROM voice_notes
JOIN voice_note_versions AS current_versions
    ON current_versions.api_key_id = voice_notes.api_key_id
    AND current_versions.voice_note_id = voice_notes.id
WHERE current_versions.version_number = (
        SELECT MAX(version_number)
        FROM voice_note_versions
        WHERE voice_note_id = voice_notes.id
    )
    AND TRIM(current_versions.text) <> ''
    AND NOT EXISTS (
        SELECT 1
        FROM embeddings
        WHERE embeddings.api_key_id = voice_notes.api_key_id
            AND embeddings.voice_note_id = voice_notes.id
    );
