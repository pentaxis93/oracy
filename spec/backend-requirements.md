# Oracy Backend Requirements

Target release: `v0.1.0`

## Exigence

A person speaks. The words matter, but the speaking is fleeting. Oracy
is the system that catches voice and keeps what was said: the user
records audio, and the system produces a **voice note** — a durable
text artifact derived from that audio that the user can search, edit,
organize, and return to later.

The voice note is the load-bearing artifact. Its data integrity is the
backend's central commitment: voice notes survive long-term on the
server, the user searches across them, the user edits them, the user
organizes them with tags and sessions. Audio is transient input that
produces a voice note; once the voice note and its embedding exist on
the server, the audio's job server-side is done. The architecture is
layered by canonicality: audio is canonical for the voice note's text
during the transcription pipeline; voice-note text is canonical for
the embedding derived from it.

This document defines what the backend must do to honor that
commitment.

## Capability

The backend accepts authenticated chunked audio submissions, composes
each submission's chunks into one transcription job's input, drives
the job to a terminal state without requiring client resubmission, and
materializes successful jobs as long-lived voice-note records.
Completed voice notes carry the data substrate `v0.1.0` requires:
current text, linear edit history, timestamped segments, server-side
embeddings, free-text tags, optional sessions, and voice-note search.
The backend exposes a per-API-key settings surface that controls which
transcription engine model is used for subsequent jobs.

## Operator Prerequisites

The backend depends on two operator-provided resources. Both are
required, not optional.

### Persistent Storage Location

- A persistent filesystem location for accepted audio chunks, composed
  audio artifacts, and other job-associated durable artifacts.
- This location is a required backend dependency, not an optional
  deployment optimization.
- The backend must not advertise durable acceptance semantics when
  this location is unavailable or not writable.
- `spec/deployment.md` must later define the operator's persistent-
  storage responsibilities, including path provisioning, persistence
  across restart, and capacity implications.

### `OPENAI_API_KEY`

- The credential that the OpenAI transcription engine authenticates
  with.
- Provided to the backend through the `OPENAI_API_KEY` environment
  variable.
- The backend must not advertise transcription capability when
  `OPENAI_API_KEY` is unset or empty.
- `spec/deployment.md` must later define the operator's responsibility
  for provisioning, rotating, and protecting this credential.

## Constraints

### Authentication and Ownership

- `v0.1.0` is a single-user testbed authenticated by operator-
  provisioned API keys.
- No public endpoint creates, rotates, revokes, or deletes API keys in
  `v0.1.0`.
- Every public resource is scoped to the authenticated API key. A key
  can only read or mutate its own jobs, voice notes, tags, sessions,
  and settings.
- Idempotency isolation is per API key. Different API keys may reuse
  the same `Idempotency-Key` without colliding.
- Tag name identity is case-insensitive within one API-key scope.
  Session identity is by session ID, not by session name.

### User Settings

- The backend exposes one per-API-key setting in `v0.1.0`:
  `transcription_model`.
- The default `transcription_model` for an API key that has never
  updated settings is `gpt-4o-mini-transcribe`.
- The selected `transcription_model` applies to every transcription
  job that reaches `queued` after the update. Jobs already past
  `queued` are unaffected.
- The backend persists settings durably and survives restart.

### Engine Surface

- The `v0.1.x` engine family is OpenAI's transcription models.
- `v0.1.0` ships two model identifiers:
  - `gpt-4o-mini-transcribe` — default. Fast, low-cost, accuracy
    suitable for typical voice notes.
  - `gpt-4o-transcribe` — quality upgrade. Higher accuracy at higher
    cost, suitable for difficult audio.
- The backend records the engine identifier in use on each `VoiceNote`
  via the `model` field, carrying the OpenAI model identifier.
- Engine identifiers in the contract are closed-enum strings whose
  internal structure carries no contract meaning. Adding or removing
  engines in future releases extends or contracts the enum but does
  not alter the shape of any endpoint or resource.
- Per-call audio size is an engine-imposed ceiling. The backend
  enforces the same per-chunk ceiling on each accepted chunk;
  `v0.1.0` documents this as `25 MiB` per chunk.

### Transcription Pipeline

#### Resource Model

- A submission attempt creates a `TranscriptionJob`, not a voice-note
  placeholder.
- A successful job creates exactly one `VoiceNote`.
- Failed and in-flight jobs do not appear in voice-note collections,
  voice-note detail, session voice-note listings, or search results.
- Voice-note resources are long-lived user artifacts. Job resources
  are workflow artifacts that may point to a voice note once complete.

#### Cost Attribution

- The `VoiceNote` resource includes a `cost_cents` field recording
  the backend's estimated cost to produce the voice note. The field
  is present on every voice-note resource; its value is nullable.
- When the backend can compute cost from engine-provided data using
  fixed-rate pricing, `cost_cents` carries a numeric estimate.
- `cost_cents` is `null` when the cost cannot be determined without
  dynamic pricing infrastructure.

#### Submission Contract (Chunked)

A submission attempt has three phases: open, push chunks, finalize.
Audio is chunked client-side so each chunk fits the per-call ceiling
of the configured transcription engine; the server composes the
chunks into the job's input.

**Open**

- Every submission requires authentication by API key.
- Every submission requires an `Idempotency-Key` on the open call,
  including web submissions.
- The open-call body declares `recorded_at`, `chunk_count`,
  `audio_format`, optional `session_id`, and optional `language`.
- `recorded_at` is client-supplied and records when the audio was
  captured.
- `chunk_count` is the number of chunks the client commits to
  pushing; it is bounded by the contract.
- `audio_format` must be one of `m4a`, `mp3`, `wav`, `webm`.
- `session_id`, when present, must identify an existing session owned
  by the authenticated API key.
- `language`, when present, is an ISO 639-1 language hint. It
  constrains the transcription request to that language and disables
  engine-side auto-detection for that submission.
- `prompt` is out of scope for `v0.1.0`.
- Pre-acceptance validation that can be performed at open time must
  remain synchronous: authentication, body fields, format, chunk-count
  bounds, language syntax, session ownership, idempotency-header
  validity.
- The open call returns a `TranscriptionJob` in status
  `accepting_chunks`. No audio bytes have been transferred yet.

**Push chunks**

- Each chunk push carries `chunk_index`, `chunk_sha256`, and the
  chunk audio bytes.
- The backend rejects chunks whose `chunk_index` is out of range,
  whose bytes exceed the per-chunk ceiling, or whose declared
  `chunk_sha256` does not match the received bytes.
- The backend persists each accepted chunk durably as it is received.
- Chunk pushes are idempotent on `(chunk_index, chunk_sha256)`.
  Re-submitting the same pair against the same job is a no-op. A push
  for a `chunk_index` that already has a different accepted hash is a
  conflict and returns `409 Conflict`, leaving the previously
  accepted chunk in place.

**Finalize**

- Finalize seals the submission attempt.
- The backend requires every declared chunk index to have an accepted
  chunk before finalize can succeed.
- The backend composes the accepted chunk hashes into the accepted
  audio content hash that participates in the idempotency tuple.
- The backend persists the composed audio artifact.
- `202 Accepted` is the durability commitment moment, not the
  per-chunk commitment.
- A request may return `202` only after the backend has durably
  persisted the job record's finalized state, the composed audio
  artifact, the accepted audio content hash, and the accepted
  `recorded_at`, `session_id`, and `language` values used for
  idempotency matching.
- If any persistence step at finalize fails, the request fails
  synchronously and the job remains in `accepting_chunks` for retry.
- Accepted (post-finalize) jobs must survive backend restart and
  continue progressing toward a terminal state. The semantic layer
  requires eventual recovery; it does not commit a numeric recovery-
  time bound.

#### Idempotency

- A submission attempt is identified by
  `API key + Idempotency-Key + accepted audio content hash +
  recorded_at + session_id + language`.
- The `Idempotency-Key` is supplied on the open call and governs
  replay matching for the entire submission attempt.
- The backend computes the accepted audio content hash at finalize
  time as a deterministic composition of the accepted chunk hashes
  in `chunk_index` order. The backend stores this composed hash and
  the accepted `recorded_at`, `session_id`, and `language` values at
  finalize time.
- The accepted submission tuple is an immutable acceptance-time
  record, not a live reference to current resource state.
- For `recorded_at`, replay matching compares the parsed instant
  normalized to UTC, not the raw wire-format string. Any RFC 3339 UTC
  representation of the same instant is a match.
- Omitted optional `session_id` and `language` values participate in
  idempotency as `null`.
- Reusing the same API key and `Idempotency-Key` with the same open-
  call body returns the original open job. Reusing them with the same
  accepted submission tuple after finalize returns the original
  finalized job.
- Reusing the same API key and `Idempotency-Key` with a mismatch on
  any open-call dimension (before finalize) or any accepted submission
  dimension (after finalize) is a client conflict and must return
  `409 Conflict`.
- When a new open call arrives under the same
  `(API key, Idempotency-Key)` as an already-terminated attempt
  (succeeded or failed), replay matching compares the new open-call
  body's `recorded_at`, `chunk_count`, `audio_format`, `session_id`,
  and `language` against the original open-call values. All match
  returns the original terminated job. Any mismatch is a client
  conflict and must return `409 Conflict`. The accepted audio content
  hash does not participate in this comparison because a fresh open
  call carries no audio bytes.
- Chunk pushes are idempotent on `(chunk_index, chunk_sha256)`.
- Deletion of a session referenced by an accepted `session_id` does
  not mutate the stored tuple or invalidate replay of that accepted
  submission attempt.
- One `Idempotency-Key` names one attempt forever. An intentional
  fresh attempt after terminal failure requires a new key; reusing
  the original key returns the same terminated job.

#### Job State Machine

Permitted states:

- `accepting_chunks`: open call accepted, awaiting chunk pushes and
  finalize
- `queued`: durably accepted at finalize, not currently being
  processed
- `processing`: actively claimed by a worker
- `retry_waiting`: waiting for the backend's scheduled retry window
- `succeeded`: terminal success, voice note persisted
- `failed`: terminal failure, no further backend retry

`accepting_chunks` is a contract-visible status. The client
legitimately needs to know whether the server is ready for finalize,
and forcing the client to track an invariant the server already knows
is unnecessary asymmetry.

Permitted transitions:

- `accepting_chunks -> queued` (finalize success)
- `accepting_chunks -> failed` (terminal pre-finalize failure such as
  abandonment timeout)
- `queued -> processing`
- `processing -> succeeded`
- `processing -> retry_waiting`
- `retry_waiting -> processing`
- `processing -> failed`

Constraints on transitions:

- A job enters `succeeded` only after its voice-note record, current
  voice-note version, segments, and current embedding have been
  durably stored.
- A job enters `failed` only for a terminal classification, after
  exhausting backend-managed retries, or — from `accepting_chunks` —
  for a terminal pre-finalize failure.
- The backend enforces a bounded abandonment window on jobs in
  `accepting_chunks`. A job that has not been finalized within that
  window transitions to terminal `failed` and is no longer eligible
  to receive chunks or finalize. This bounds the lifetime of
  unfinalized submission state. The specific window value and the
  precise abandonment definition are implementation-tunable and not
  contract-visible; clients observe only the eventual state
  transition through the `TranscriptionJob` resource.
- `processing` is leased work, not a terminal assumption. A crashed
  worker or backend restart must allow the job to be reclaimed and
  resumed.

#### Retry Ownership

- Once a job has been accepted (post-finalize), the backend owns
  transient retries.
- The client does not resubmit while the job is nonterminal.
- The job resource must expose `retry_count` on every read.
- The job resource must expose `next_attempt_at` when the job is in
  `retry_waiting` so the client can suppress wasteful polling during
  a known backoff window.
- The job resource must expose `chunks_received` while the job is in
  `accepting_chunks` so the client can render upload progress and
  decide when to call `finalize`.

#### Failure Semantics

Terminal failure codes:

- `audio_invalid`
- `engine_timeout`
- `engine_rate_limited`
- `engine_error`
- `storage_error`
- `internal_error`
- `submission_abandoned`

Semantics:

- `audio_invalid` means the accepted payload cannot be transcribed
  and further client retry of the same audio is not meaningful.
- `engine_timeout`, `engine_rate_limited`, `engine_error`,
  `storage_error`, and `internal_error` are retryable classes during
  backend-owned retry, but may still become terminal after retry
  exhaustion.
- `submission_abandoned` means the submission attempt was opened but
  was not finalized within the backend's abandonment window; the
  partial submission has been released and the job is terminal.
- `retryable_by_client=false` only for `audio_invalid`.
- `retryable_by_client=true` for the other terminal classes
  (including `submission_abandoned`) after backend retries have been
  exhausted; for `submission_abandoned`, the user's path forward is
  to record again with a new `Idempotency-Key`.

#### Ordering and Visibility

- Job claim order is FIFO by readiness: first by retry eligibility,
  then by acceptance (finalize) time.
- Completion order is not guaranteed and is not exposed as a contract.
- Voice-note history is ordered newest-first by voice-note
  `created_at`, with descending `id` as the deterministic tiebreaker.

### Data Substrate

#### VoiceNote

- The `VoiceNote` resource includes `id`, `current_version_id`,
  `text`, `audio_duration_seconds`, `audio_format`,
  `audio_size_bytes`, `language`, `model`, `processing_time_ms`,
  `cost_cents`, `created_at`, `recorded_at`, `session_id`, and
  `tags`.
- `text` is always the current voice-note version's text.
- `current_version_id` points to the latest `VoiceNoteVersion`.
- `created_at` records when the voice note was first materialized
  from a successful job.
- `recorded_at` records when the client says the audio was captured.
- `audio_duration_seconds`, `audio_format`, and `audio_size_bytes`
  are durable provenance properties of the voice note. They survive
  even after the composed audio bytes have been released from the
  server at the originating job's terminal state.
- `language` is the language hint used for the transcription, or the
  detected language when no hint was supplied.
- `model` carries the OpenAI engine identifier in use when the voice
  note was produced.
- `session_id` is nullable.
- `tags` is an array of `Tag` resources owned by the authenticated
  API key.

#### VoiceNoteVersion

- The `VoiceNoteVersion` resource includes `id`, `voice_note_id`,
  `text`, and `created_at`.
- Version history is linear and append-only. No branching or merging
  exists in `v0.1.0`.
- The initial transcription result becomes the first voice-note
  version.
- Each text edit creates one new `VoiceNoteVersion` and moves
  `VoiceNote.current_version_id` to that new version.
- Historical versions remain readable through the version-history
  endpoint.

#### Segment

- The `Segment` resource includes `id`, `voice_note_id`, `position`,
  `start_ms`, `end_ms`, and `text`.
- Segments are stored as first-class rows joined to the voice note,
  not as a blob embedded in voice-note text.
- Segment ordering is ascending by `position`.
- `start_ms` and `end_ms` are measured from the start of the composed
  audio and represent engine-ground-truth timing. Segments are not
  user-editable in `v0.1.0`.
- Segments remain anchored to the voice note across voice-note text
  edits.
- Speaker diarization is out of scope. No `speaker_label` field
  exists in `v0.1.0`.

#### Tag

- The `Tag` resource includes `id`, `name`, and `created_at`.
- Tags are free-text and many-to-many with voice notes.
- Tag identity is case-insensitive within one API-key scope.
- The stored `name` preserves the spelling of the latest create or
  rename request that established the current tag value.
- Creating a tag with a case-insensitive name that already exists for
  the authenticated API key returns the existing `Tag` instead of
  creating a duplicate.
- Renaming a tag updates the visible name on every associated voice
  note immediately because associations are by `tag_id`, not copied
  text.

#### Session

- The `Session` resource includes `id`, `name`, and `created_at`.
- Sessions are optional groupings for related voice notes.
- A voice note may belong to at most one session in `v0.1.0`.
- `session_id=null` is valid and means the voice note is ungrouped.
- Session identity is by ID. Session names are not required to be
  unique.

#### Embeddings

- The backend stores one current embedding per voice note.
- The first embedding is generated when a successful transcription
  job creates the voice note.
- A text edit triggers asynchronous regeneration of the voice note's
  current embedding.
- Full-text reads reflect the new voice-note text immediately after
  the edit is committed.
- Semantic and hybrid search may return stale matches until embedding
  regeneration completes.
- Embeddings are not exposed on the default voice-note resource and
  no public endpoint returns embedding vectors in `v0.1.0`.

#### Editing Scope

- Editing is the only adjustment path for an existing voice note.
- The backend does not re-transcribe audio against an existing voice
  note. Once a voice note exists, it is final from the transcription
  pipeline's perspective; subsequent quality concerns are addressed
  through editing.
- If a transcription job fails terminally before a voice note is
  produced, the user's path forward is to record again or re-upload
  from local storage. The backend treats the new submission as a
  fresh transcription attempt, producing a new voice-note identity if
  successful.

#### Settings

- The `Settings` resource includes `transcription_model`.
- The `transcription_model` value is one of the documented engine
  identifiers (`gpt-4o-mini-transcribe`, `gpt-4o-transcribe`).
- The default at first read for an API key that has never updated
  settings is `gpt-4o-mini-transcribe`.

### Search and Retrieval

- Voice-note history and voice-note search share one collection
  contract: `GET /api/v1/voice-notes`.
- Without `q`, the collection is voice-note history with optional
  filters.
- With `q`, the collection is voice-note search with the same result
  shape, filters, and pagination. This is an explicit `v0.1.0`
  governance decision to avoid duplicating voice-note collection
  surface with no semantic gain.
- Search results are always `VoiceNote` resources. Jobs, sessions,
  tags, versions, and segments are not returned as search hits.
- Keyword search supports full-text matching over current voice-note
  text and may additionally match historical voice-note-version text,
  but results always resolve to the parent `VoiceNote`.
- Semantic search uses the current voice-note embedding.
- Hybrid search combines keyword and semantic ranking over the same
  voice-note result set.
- When `q` is present and `search_mode` is omitted, the backend
  defaults to `hybrid`.
- Search filters support tag, session, `recorded_at` time range, and
  `created_at` time range. `*_after` bounds are exclusive;
  `*_before` bounds are inclusive.
- Repeated `tag_id` filters combine by intersection: a voice note
  matches only if it is associated with all supplied tags.
- Voice-note history is ordered newest-first by voice-note
  `created_at`, with descending `id` as the deterministic tiebreaker.
- Version-history listings are ordered newest-first by `created_at`,
  with descending `id` as the deterministic tiebreaker.
- Tag listings are ordered newest-first by `created_at`, with
  descending `id` as the deterministic tiebreaker.
- Session listings are ordered newest-first by `created_at`, with
  descending `id` as the deterministic tiebreaker.
- Search results are ordered by backend relevance score descending,
  then by voice-note `created_at` descending, then by descending `id`
  as the final deterministic tiebreaker.
- Voice-note collection pagination is cursor-based. The default page
  size is `50`; the maximum page size is `100`.
- Voice-note, session, tag, version-history, and segment listings use
  the same collection envelope: `items` plus nullable `next_cursor`.

### Mutation Semantics

- `PATCH /api/v1/voice-notes/{voice_note_id}` changes voice-note text
  only.
- A voice-note edit creates a new `VoiceNoteVersion`; it does not
  rewrite or delete prior versions.
- `PUT /api/v1/voice-notes/{voice_note_id}/tags` replaces the voice
  note's entire tag set with the supplied `tag_ids`.
- Replacing tags does not create a new `VoiceNoteVersion`.
- Session membership is assigned only at transcription submission
  time in `v0.1.0`. No later voice-note-to-session reassignment
  endpoint exists.
- `PATCH /api/v1/settings` is a partial update; omitted fields are
  left unchanged.

### Deletion and Retention

- Voice-note retention is indefinite until explicit voice-note
  deletion.
- Voice-note deletion is a hard delete with no grace period.
- Deleting a voice note cascades to its versions, segments, current
  embedding, and tag associations.
- The originating `TranscriptionJob` survives voice-note deletion as
  the durable replay record for the accepted submission attempt.
- If a deleted voice note came from a succeeded job, that job remains
  `succeeded` and its `voice_note_id` becomes `null`.
- Deleting a session sets `session_id` to `null` on contained voice
  notes. The voice notes remain otherwise unchanged.
- Deleting a session does not invalidate replay records for
  submissions that were accepted with that `session_id`.
- Deleting a tag removes its voice-note associations. The voice notes
  remain otherwise unchanged.
- The backend retains accepted audio chunks and composed audio only
  while a job is in `accepting_chunks`, `queued`, `processing`, or
  `retry_waiting`.
- Once a job reaches `succeeded` or `failed`, the backend must
  promptly delete the retained audio chunks and composed audio.
- Failure to delete retained audio does not create a new public job
  state. The job remains terminal while cleanup is retried internally
  and surfaced to operators through logs and metrics.

### Error Contract

- Every `4xx` and `5xx` response body is JSON with at least
  `error_code` and `message`.
- `details` is optional and carries structured validation or conflict
  context when useful.
- Validation errors use `details` to identify request fields that
  failed validation.
- Error response structure is shared across job, voice-note, tag,
  session, and settings endpoints.

## Acceptance

The backend requirements are acceptable for `v0.1.0` when the
following are true:

- The operator's persistent storage location and `OPENAI_API_KEY` are
  named as required prerequisites, and the backend refuses to
  advertise the dependent capabilities when either is missing.
- A valid open call returns a `TranscriptionJob` in
  `accepting_chunks` with the declared `chunk_count`.
- Each accepted chunk push moves `chunks_received` toward
  `chunk_count`, and the chunk push is idempotent on
  `(chunk_index, chunk_sha256)`.
- A finalize call after every declared chunk has been accepted
  returns `202 Accepted` and durably commits the job, the composed
  audio, and the accepted submission tuple before returning.
- An `accepting_chunks` job that is not finalized within the
  backend's abandonment window transitions to terminal `failed`
  rather than persisting indefinitely.
- Replaying a submission with the same API key, `Idempotency-Key`,
  and accepted submission tuple returns the same job in every state.
- Reusing an accepted `Idempotency-Key` with a mismatch on any
  open-call or accepted submission dimension returns `409 Conflict`
  and leaves the original job unchanged.
- Accepted (post-finalize) jobs survive backend restart and still
  reach a terminal state.
- Backend-managed retries move transient failures through
  `retry_waiting` instead of forcing client resubmission.
- `retry_count` is always visible on the job resource;
  `next_attempt_at` is visible during `retry_waiting`;
  `chunks_received` is visible during `accepting_chunks`.
- Successful jobs create one voice note each, and only completed
  voice notes appear in voice-note collections, detail, session
  voice-note listings, and search.
- Each `VoiceNote.model` carries the OpenAI engine identifier in use
  when the voice note was produced.
- The settings resource exposes `transcription_model` per API key
  with the documented default; updates apply to jobs that reach
  `queued` after the update; settings persist across restart.
- Deleting a voice note does not delete its originating job, and
  replay of the same accepted submission tuple still returns that
  original job.
- The voice-note substrate is concrete: voice notes carry current
  text, sessions, tags, and current version identity; versions are
  append-only; segments are first-class timing rows; embeddings are
  stored server-side and regenerated asynchronously after text edits.
- Editing is the only adjustment path for an existing voice note;
  the backend does not re-transcribe against an existing voice note.
- Search behavior is explicit: history and search share one voice-
  note collection contract, omitted `search_mode` with `q` defaults
  to `hybrid`, repeated `tag_id` filters intersect, results are
  voice-note-only, and semantic search is eventually consistent
  after edits.
- Deletion semantics are explicit and match resource ownership
  expectations.
- Audio retention semantics are explicit: chunks and composed audio
  are released promptly once the originating job reaches `succeeded`
  or `failed`.
- Supported audio formats, per-chunk size ceiling, language-hint
  semantics, pagination shape, and error-envelope semantics are
  named concretely.
- No vendor product names appear in contract shapes or response
  field names outside the engine surface section and the
  `VoiceNote.model` field, and no implementation substrate leaks
  into the backend requirements.
