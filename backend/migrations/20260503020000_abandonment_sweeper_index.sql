CREATE INDEX transcription_jobs_abandonment_idx
    ON transcription_jobs (status, created_at, id);
