ALTER TABLE transcription_jobs
ADD COLUMN accepted_at TEXT;

UPDATE transcription_jobs
SET accepted_at = updated_at
WHERE status IN ('queued', 'processing', 'retry_waiting', 'succeeded', 'failed')
    AND accepted_at IS NULL;

DROP INDEX IF EXISTS transcription_jobs_ready_idx;
DROP INDEX IF EXISTS transcription_jobs_processing_lease_idx;

CREATE INDEX transcription_jobs_ready_idx
    ON transcription_jobs (status, next_attempt_at, accepted_at, id);

CREATE INDEX transcription_jobs_processing_lease_idx
    ON transcription_jobs (status, processing_lease_expires_at, next_attempt_at, accepted_at, id);
