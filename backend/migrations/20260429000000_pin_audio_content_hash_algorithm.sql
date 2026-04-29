ALTER TABLE transcription_jobs
    ADD COLUMN audio_content_hash_algorithm TEXT NOT NULL
    DEFAULT 'sha256:chunk-sha256-raw-concat:v1';

DROP TRIGGER transcription_jobs_accepted_tuple_immutable;

CREATE TRIGGER transcription_jobs_accepted_tuple_immutable
BEFORE UPDATE OF api_key_id, idempotency_key, audio_sha256_hex, audio_content_hash_algorithm, recorded_at, session_id, language
ON transcription_jobs
BEGIN
    SELECT RAISE(ABORT, 'accepted submission tuple is immutable');
END;
