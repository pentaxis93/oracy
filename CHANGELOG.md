# Changelog

## Unreleased

- Rename the transcript schema field from `whisper_model` to `model` so the API
  describes the transcription engine without binding the contract to Whisper.
- Clarify that `cost_cents` is always present on transcript resources but may be
  `null` when the backend cannot derive a stable estimate from fixed-rate
  pricing inputs.
