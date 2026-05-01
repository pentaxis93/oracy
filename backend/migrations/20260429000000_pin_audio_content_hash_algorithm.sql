ALTER TABLE transcription_jobs
    ADD COLUMN audio_content_hash_algorithm TEXT NOT NULL
    DEFAULT 'sha256:chunk-sha256-raw-concat:v1';

DROP TRIGGER transcription_jobs_accepted_tuple_immutable;

CREATE TRIGGER transcription_jobs_accepted_tuple_immutable
BEFORE UPDATE OF api_key_id, idempotency_key, audio_sha256_hex, audio_content_hash_algorithm, recorded_at, session_id, language, audio_format, chunk_count
ON transcription_jobs
WHEN NEW.api_key_id IS NOT OLD.api_key_id
    OR NEW.idempotency_key IS NOT OLD.idempotency_key
    OR (OLD.audio_sha256_hex != '' AND NEW.audio_sha256_hex IS NOT OLD.audio_sha256_hex)
    OR NEW.audio_content_hash_algorithm IS NOT OLD.audio_content_hash_algorithm
    OR NEW.recorded_at IS NOT OLD.recorded_at
    OR NEW.session_id IS NOT OLD.session_id
    OR NEW.language IS NOT OLD.language
    OR NEW.audio_format IS NOT OLD.audio_format
    OR NEW.chunk_count IS NOT OLD.chunk_count
BEGIN
    SELECT RAISE(ABORT, 'accepted submission tuple is immutable');
END;
