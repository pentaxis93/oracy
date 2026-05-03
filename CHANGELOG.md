# Changelog

## Unreleased

- Add a transcription-job abandonment sweeper that fails stale
  `accepting_chunks` jobs with `submission_abandoned` and reports swept jobs
  through operator logs and metrics.
- Release retained transcription audio when jobs reach `succeeded` or `failed`,
  retry cleanup after transient release failures, and surface per-artifact
  cleanup outcomes through structured logs and retention metrics.
- Add an operator-only Prometheus metrics listener with initial worker,
  retention-cleanup, retained-audio capacity metrics, and overlapping bind
  validation.
- Harden transcription worker reliability so stalled OpenAI requests enter the
  backend retry path, processing leases renew during long transcriptions,
  successful retries expose no stale failure metadata, failed sliced
  transcriptions release generated slice files, and queued chunked jobs are
  claimed by finalize order.
- Add authenticated chunked transcription-job submission APIs for opening
  attempts, accepting durable chunks, finalizing composed audio to queued jobs,
  and reading upload progress.
- Add the backend transcription-job processing worker, including leased job
  claiming, OpenAI transcription requests, FFmpeg-backed audio slicing,
  coarse segment materialization, backend-owned retries, and terminal failure
  classification. Until #27 lands, this implementation can reach `succeeded`
  after voice-note/version/segment materialization without a current embedding;
  that remains a v0.1.0 release-blocker deviation.
- Add authenticated tag and session management APIs, including owner-scoped
  CRUD, shared cursor pagination, case-insensitive tag identity with
  latest-spelling display updates, and metadata deletion cascades.
- Add authenticated voice-note history, detail, version-history, segment, and
  session-scoped history read APIs. Collection filters compose across tags,
  sessions, and recorded/created time ranges; search query parameters
  conservatively return no results until the tracked search work lands.
- Define the backend runtime commitment not to emit the literal
  `OPENAI_API_KEY` value through backend-controlled logs, diagnostics, or error
  surfaces.
- Add the operator-facing deployment contract for backend accepted-audio
  storage, SQLite persistence, and `OPENAI_API_KEY` provisioning.
- Preserve the shared `ErrorResponse` envelope for framework-owned API
  rejections, including unsupported methods and JSON body parse, content-type,
  shape, and size failures.
- Add authenticated `GET /api/v1/settings` and `PATCH /api/v1/settings`
  endpoints for durable per-API-key transcription model settings.
- Require a non-empty `OPENAI_API_KEY` during backend startup so the service does
  not advertise transcription capability without its operator-provisioned engine
  credential.
- Document the `TranscriptionJob.failure_code` wire enum in the API
  contract while keeping per-code failure semantics in backend
  requirements.
- Fix the foreground upload queue so pending recordings wait for a configured
  API key and resume after one is saved.
- Add the durable backend SQLite substrate for accepted transcription jobs,
  voice-note versions, ordered segments, current embeddings, owner scoping, and
  the required operator `database_path` setting.
- Add the durable backend SQLite substrate for tags, sessions,
  voice-note-to-tag associations, voice-note session membership, and metadata
  deletion invariants.
- Establish independent backend and frontend CI gates for v0.1.0 pull requests
  and pushes to `main`.
- Add the initial Rust backend runtime with explicit startup validation for
  operator-provisioned API keys, durable accepted-audio storage, shared JSON
  errors, and bearer authentication.
- Import the Flutter client into `client/` with six inherited defects fixed
  in-place rather than deferred, and commit `client/pubspec.lock` for
  reproducible app builds.
- Expand the v0.1.0 spec to define the full voice-note substrate and HTTP
  surface, including sessions, tags, voice-note editing and version history,
  segment retrieval, voice-note deletion, unified history/search collection
  semantics, required `recorded_at`, and the shared error envelope.
- Rename the voice-note schema field from `whisper_model` to `model` so the API
  describes the transcription engine without binding the contract to Whisper.
- Clarify that `cost_cents` is always present on voice-note resources but may be
  `null` when the backend cannot derive a stable estimate from fixed-rate
  pricing inputs.
- Rewrite the v0.1.0 spec around the durable voice-note artifact: rename the
  resource family, replace single-multipart audio upload with the chunked
  open/push/finalize protocol (with `accepting_chunks` as a contract-visible job
  status), and add the per-API-key `transcription_model` settings surface
  enumerating the OpenAI transcription engine identifiers.
