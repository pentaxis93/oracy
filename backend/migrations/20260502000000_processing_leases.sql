ALTER TABLE transcription_jobs
ADD COLUMN processing_lease_token TEXT;

ALTER TABLE transcription_jobs
ADD COLUMN processing_lease_expires_at TEXT;

CREATE INDEX transcription_jobs_processing_lease_idx
    ON transcription_jobs (status, processing_lease_expires_at, next_attempt_at, created_at, id);
