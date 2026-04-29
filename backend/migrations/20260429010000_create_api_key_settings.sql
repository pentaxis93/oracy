CREATE TABLE api_key_settings (
    api_key_id TEXT PRIMARY KEY,
    transcription_model TEXT NOT NULL CHECK (
        transcription_model IN (
            'gpt-4o-mini-transcribe',
            'gpt-4o-transcribe'
        )
    ),
    updated_at TEXT NOT NULL
);
