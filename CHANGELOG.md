# Changelog

## Unreleased

- Make the Flutter client's API base URL configurable through
  `ORACY_API_BASE_URL` build defaults and a runtime Settings override, with
  credential clearing when the effective server URL changes.
- Document supported reverse-proxy networking patterns for backend Quadlet
  deployments, including shared container networks, Linux Docker host-gateway
  setup, and firewall requirements for non-loopback public API publishes.
- Fix the backend Quadlet template's SELinux handling so dedicated
  config and state mounts work under default confined Podman on
  SELinux-enforcing hosts.
- Add backend container and Quadlet deployment templates, including
  bind-backed persistence, example configuration, loopback-default operator
  metrics publishing, and graceful SIGTERM shutdown.
- Clarify that `VoiceNote.language` may be `null` in the API contract when no
  language hint or detected language is available.
- Refresh queued Flutter transcription idempotency keys when a saved language
  hint changes, preventing backend replay conflicts on the fresh submission.
- Fix Flutter foreground transcription provenance so web recordings persist the
  recording start time, and treat voice notes deleted after accepted
  transcription as accepted work instead of failed uploads.
- Migrate the Flutter client's submission flow to the v0.1.0 chunked
  transcription-job protocol, including client-side chunking, SHA-256 chunk
  hashes, async job polling, retry-window suppression, and final voice-note
  fetches.
- Migrate the Flutter client's history/search read side to the v0.1.0
  voice-note collection contract, including `{items, next_cursor}` parsing,
  cursor pagination, voice-note resource modeling, and voice-note history UI
  copy.
- Preserve backend relevance order when rendering Flutter history search
  results while keeping date grouping for normal history browsing.
- Fix Flutter history parsing and rendering for voice notes whose `language`
  field is null.
- Add a repo-defined backend CI-parity gate for contributor pre-review checks.
- Fix semantic and hybrid voice-note search over large filtered candidate sets
  so embedding lookup no longer fails on SQLite host-parameter limits.
- Add ranked voice-note search across keyword, semantic, and hybrid modes,
  including historical-version keyword matches, current-embedding semantic
  ranking, literal full-text query handling, independent semantic and keyword
  gates, relevance cursors, and the shared voice-note collection filters.
- Return empty semantic search results without calling the embedding provider
  when local filters leave no current embeddings to rank, preserving hybrid
  keyword results in the same case.
- Bound keyword-only search pagination so broad keyword matches return only
  the requested page from storage instead of materializing the full match set.
- Add OpenAI-backed voice-note embedding generation, long-text chunk pooling,
  durable edit-triggered embedding regeneration, and the
  `succeeded`-requires-current-embedding workflow gate. Blank voice-note text
  updates are rejected so every current embedding has non-empty source text.
- Add authenticated voice-note mutation APIs for text replacement with
  version history, full tag-set replacement, hard deletion with cascades, and
  edit-triggered embedding-regeneration initiation.
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
  classification.
- Add authenticated tag and session management APIs, including owner-scoped
  CRUD, shared cursor pagination, case-insensitive tag identity with
  latest-spelling display updates, and metadata deletion cascades.
- Add authenticated voice-note history, detail, version-history, segment, and
  session-scoped history read APIs. Collection filters compose across tags,
  sessions, and recorded/created time ranges.
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
