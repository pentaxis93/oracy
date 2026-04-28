# Oracy Backend

`backend/` contains the Rust service for Oracy's `v0.1.0` backend. The
backend targets Linux deployment and assumes POSIX filesystem semantics.

Run the service with `ORACY_CONFIG` set to a TOML configuration file and
`OPENAI_API_KEY` set to a valid OpenAI credential:

```toml
listen_addr = "127.0.0.1:8080"
accepted_audio_dir = "/var/lib/oracy/accepted-audio"
database_path = "/var/lib/oracy/oracy.sqlite"

[[api_keys]]
api_key_id = "operator-issued-id"
key = "operator-issued-secret"
```

Startup fails unless `api_keys` contains at least one valid operator-provisioned
key, `accepted_audio_dir` already exists as a writable directory, and
`database_path` points to a SQLite database file whose parent directory exists
and is writable. Relative storage paths resolve from the real configuration
file directory.

Transcription requires `OPENAI_API_KEY` to be set in the environment; without
it, transcription jobs cannot succeed.

Every API route requires `Authorization: Bearer <api_key>`. The bearer scheme is
case-insensitive, and missing or invalid keys return the shared JSON
`ErrorResponse` with `401 Unauthorized`.
