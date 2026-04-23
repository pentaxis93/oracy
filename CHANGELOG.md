# Changelog

## Unreleased

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
