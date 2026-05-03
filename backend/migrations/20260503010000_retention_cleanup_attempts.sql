ALTER TABLE transcription_jobs
ADD COLUMN accepted_audio_cleanup_attempts INTEGER NOT NULL DEFAULT 0 CHECK (accepted_audio_cleanup_attempts >= 0);

ALTER TABLE transcription_job_chunks
ADD COLUMN cleanup_attempts INTEGER NOT NULL DEFAULT 0 CHECK (cleanup_attempts >= 0);

CREATE INDEX transcription_jobs_terminal_retained_audio_idx
    ON transcription_jobs (status, accepted_audio_path);

CREATE INDEX transcription_job_chunks_retained_audio_idx
    ON transcription_job_chunks (chunk_path);
