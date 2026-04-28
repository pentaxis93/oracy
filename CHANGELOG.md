# Changelog

## Unreleased

- Add the durable backend SQLite substrate for accepted transcription jobs,
  transcript versions, ordered segments, current embeddings, owner scoping, and
  the required operator `database_path` setting.
- Add the durable backend SQLite substrate for tags, sessions,
  transcript-to-tag associations, transcript session membership, and metadata
  deletion invariants.
- Establish independent backend and frontend CI gates for v0.1.0 pull requests
  and pushes to `main`.
- Add the initial Rust backend runtime with explicit startup validation for
  operator-provisioned API keys, durable accepted-audio storage, shared JSON
  errors, and bearer authentication.
- Import the Flutter client into `client/` with six inherited defects fixed
  in-place rather than deferred, and commit `client/pubspec.lock` for
  reproducible app builds.
- Expand the v0.1.0 spec to define the full transcript substrate and HTTP
  surface, including sessions, tags, transcript editing and version history,
  segment retrieval, transcript deletion, unified history/search collection
  semantics, required `recorded_at`, and the shared error envelope.
- Rename the transcript schema field from `whisper_model` to `model` so the API
  describes the transcription engine without binding the contract to Whisper.
- Clarify that `cost_cents` is always present on transcript resources but may be
  `null` when the backend cannot derive a stable estimate from fixed-rate
  pricing inputs.
- Rewrite the v0.1.0 spec around the durable voice-note artifact: rename the
  `Transcript` family to `VoiceNote`, replace single-multipart audio upload with
  the chunked open/push/finalize protocol (with `accepting_chunks` as a
  contract-visible job status), and add the per-API-key `transcription_model`
  settings surface enumerating the OpenAI transcription engine identifiers.
