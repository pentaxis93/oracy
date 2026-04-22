# Oracy Backend Requirements

Target release: `v0.1.0`

## Capability

The backend accepts authenticated audio submissions, creates durable
transcription jobs, drives those jobs to a terminal state without requiring
client resubmission, and materializes successful jobs as long-lived transcript
records. Completed transcripts carry the data substrate required for `v0.1.0`:
current text, linear edit history, timestamped segments, server-side
embeddings, free-text tags, optional sessions, and transcript search.

## Constraints

### Authentication and Ownership

- `v0.1.0` is a single-user testbed authenticated by operator-provisioned API
  keys.
- No public endpoint creates, rotates, revokes, or deletes API keys in
  `v0.1.0`.
- Every public resource is scoped to the authenticated API key. A key can only
  read or mutate its own jobs, transcripts, tags, and sessions.
- Idempotency isolation is per API key. Different API keys may reuse the same
  `Idempotency-Key` without colliding.
- Tag name identity is case-insensitive within one API-key scope. Session
  identity is by session ID, not by session name.

### Transcription Pipeline

#### Resource Model

- A submission creates a `TranscriptionJob`, not a transcript placeholder.
- A successful job creates exactly one `Transcript`.
- Failed and in-flight jobs do not appear in transcript history, transcript
  detail, session transcript listings, or search results.
- Transcript resources are long-lived user artifacts. Job resources are
  workflow artifacts that may point to a transcript once complete.

#### Cost Attribution

- The `Transcript` resource includes a `cost_cents` field recording the
  backend's estimated cost to produce the transcript. The field is present on
  every transcript resource; its value is nullable.
- When the backend can compute cost from engine-provided data using fixed-rate
  pricing, `cost_cents` carries a numeric estimate.
- `cost_cents` is `null` when the cost cannot be determined without dynamic
  pricing infrastructure.

#### Durable Acceptance

- `202 Accepted` is a durability commitment, not an admission of best effort.
- A request may return `202` only after the backend has durably persisted the
  job record, the accepted audio payload, the accepted audio content hash, and
  the accepted `recorded_at`, `session_id`, and `language` values used for
  idempotency matching.
- If any persistence step fails, the request fails synchronously and no
  accepted job exists.
- Accepted jobs must survive backend restart and continue progressing toward a
  terminal state. The semantic layer requires eventual recovery, but it does
  not commit a numeric recovery-time bound.

#### Operator Dependency

- Durable acceptance depends on an operator-provided persistent storage
  location for accepted audio and other job-associated durable artifacts.
- This storage location is a required backend dependency, not an optional
  deployment optimization.
- The backend must not advertise durable acceptance semantics when that storage
  location is unavailable or not writable.
- `spec/deployment.md` must later define the operator's persistent-storage
  responsibilities, including path provisioning, persistence across restart,
  and capacity implications.

#### Submission Contract

- Every submission requires authentication by API key.
- Every submission requires an `Idempotency-Key`, including web submissions.
- The request body contains the uploaded audio file, required `recorded_at`,
  optional `session_id`, and optional `language`.
- `recorded_at` is client-supplied and records when the audio was captured.
- `language`, when present, is an ISO 639-1 language hint. It constrains the
  transcription request to that language and disables engine-side
  auto-detection for that submission.
- `session_id`, when present, must identify an existing session owned by the
  authenticated API key.
- Supported upload formats are `m4a`, `mp3`, `wav`, and `webm`.
- The maximum accepted upload size is `25 MiB`.
- `prompt` is out of scope for `v0.1.0`.
- Validation that can be performed before acceptance must remain synchronous:
  authentication, filename presence, supported format, file-size limit,
  `recorded_at` syntax, session ownership, language syntax, and
  idempotency-header validity.

#### Idempotency

- One submission attempt is identified by `API key + Idempotency-Key +
  accepted audio content hash + recorded_at + session_id + language`.
- The backend computes and stores the accepted audio content hash and the
  accepted `recorded_at`, `session_id`, and `language` values at acceptance
  time.
- Omitted optional `session_id` and `language` values participate in
  idempotency as `null`.
- Reusing the same API key and `Idempotency-Key` with the same accepted
  submission tuple returns the original job instead of creating duplicate work.
- Reusing the same API key and `Idempotency-Key` with a mismatch on any
  accepted submission dimension is a client conflict and must return
  `409 Conflict`.
- One `Idempotency-Key` names one attempt forever. Reusing it after terminal
  failure returns the same failed job. An intentional fresh attempt requires a
  new key.

#### Job State Machine

Permitted states:

- `queued`: accepted, durable, not currently being processed
- `processing`: actively claimed by a worker
- `retry_waiting`: waiting for the backend's scheduled retry window
- `succeeded`: terminal success, transcript persisted
- `failed`: terminal failure, no further backend retry

Permitted transitions:

- `queued -> processing`
- `processing -> succeeded`
- `processing -> retry_waiting`
- `retry_waiting -> processing`
- `processing -> failed`

Constraints on transitions:

- A job enters `succeeded` only after its transcript record, current
  transcript version, segments, and current embedding have been durably stored.
- A job enters `failed` only for a terminal classification or after exhausting
  backend-managed retries.
- `processing` is leased work, not a terminal assumption. A crashed worker or
  backend restart must allow the job to be reclaimed and resumed.

#### Retry Ownership

- Once a job is accepted, the backend owns transient retries.
- The client does not resubmit while the job is nonterminal.
- The job resource must expose `retry_count` on every read.
- The job resource must expose `next_attempt_at` when the job is in
  `retry_waiting` so the client can suppress wasteful polling during a known
  backoff window.

#### Failure Semantics

Terminal failure codes:

- `audio_invalid`
- `engine_timeout`
- `engine_rate_limited`
- `engine_error`
- `storage_error`
- `internal_error`

Semantics:

- `audio_invalid` means the accepted payload cannot be transcribed and further
  client retry of the same audio is not meaningful.
- `engine_timeout`, `engine_rate_limited`, `engine_error`, `storage_error`,
  and `internal_error` are retryable classes during backend-owned retry, but
  may still become terminal after retry exhaustion.
- `retryable_by_client=false` only for `audio_invalid`.
- `retryable_by_client=true` for the other terminal classes after backend
  retries have been exhausted.

#### Ordering and Visibility

- Job claim order is FIFO by readiness: first by retry eligibility, then by
  acceptance time.
- Completion order is not guaranteed and is not exposed as a contract.
- Transcript history is ordered newest-first by transcript `created_at`.

### Data Substrate

#### Transcript

- The `Transcript` resource includes `id`, `current_version_id`, `transcript`,
  `audio_duration_seconds`, `audio_format`, `audio_size_bytes`,
  `transcript_language`, `model`, `processing_time_ms`, `cost_cents`,
  `created_at`, `recorded_at`, `session_id`, and `tags`.
- `transcript` is always the current transcript version's text.
- `current_version_id` points to the latest `TranscriptVersion`.
- `created_at` records when the transcript was first materialized from a
  successful job.
- `recorded_at` records when the client says the audio was captured.
- `session_id` is nullable.
- `tags` is an array of `Tag` resources owned by the authenticated API key.

#### TranscriptVersion

- The `TranscriptVersion` resource includes `id`, `transcript_id`,
  `transcript`, and `created_at`.
- Version history is linear and append-only. No branching or merging exists in
  `v0.1.0`.
- The initial transcription result becomes the first transcript version.
- Each text edit creates one new `TranscriptVersion` and moves
  `Transcript.current_version_id` to that new version.
- Historical versions remain readable through the version-history endpoint.

#### Segment

- The `Segment` resource includes `id`, `transcript_id`, `position`,
  `start_ms`, `end_ms`, and `text`.
- Segments are stored as first-class rows joined to the transcript, not as a
  blob embedded in transcript text.
- Segment ordering is ascending by `position`.
- Segments represent engine-ground-truth timing. They are not user-editable in
  `v0.1.0`.
- Segments remain anchored to the transcript across transcript text edits.
- Speaker diarization is out of scope. No `speaker_label` field exists in
  `v0.1.0`.

#### Tag

- The `Tag` resource includes `id`, `name`, and `created_at`.
- Tags are free-text and many-to-many with transcripts.
- Tag identity is case-insensitive within one API-key scope.
- The stored `name` preserves the spelling of the latest create or rename
  request that established the current tag value.
- Creating a tag with a case-insensitive name that already exists for the
  authenticated API key returns the existing `Tag` instead of creating a
  duplicate.
- Renaming a tag updates the visible name on every associated transcript
  immediately because associations are by `tag_id`, not copied text.

#### Session

- The `Session` resource includes `id`, `name`, and `created_at`.
- Sessions are optional groupings for related recordings.
- A transcript may belong to at most one session in `v0.1.0`.
- `session_id=null` is valid and means the transcript is ungrouped.
- Session identity is by ID. Session names are not required to be unique.

#### Embeddings

- The backend stores one current embedding per transcript.
- The first embedding is generated when a successful transcription job creates
  the transcript.
- A text edit triggers asynchronous regeneration of the transcript's current
  embedding.
- Full-text reads reflect the new transcript text immediately after the edit is
  committed.
- Semantic and hybrid search may return stale matches until embedding
  regeneration completes.
- Embeddings are not exposed on the default transcript resource and no public
  endpoint returns embedding vectors in `v0.1.0`.

### Search and Retrieval

- Transcript history and transcript search share one collection contract:
  `GET /api/v1/transcripts`.
- Without `q`, the collection is transcript history with optional filters.
- With `q`, the collection is transcript search with the same result shape,
  filters, and pagination. This is an explicit `v0.1.0` governance decision to
  avoid duplicating transcript-collection surface with no semantic gain.
- Search results are always `Transcript` resources. Jobs, sessions, tags,
  versions, and segments are not returned as search hits.
- Keyword search supports full-text matching over current transcript text and
  may additionally match historical transcript-version text, but results always
  resolve to the parent `Transcript`.
- Semantic search uses the current transcript embedding.
- Hybrid search combines keyword and semantic ranking over the same transcript
  result set.
- Search filters support tag, session, `recorded_at` time range, and
  `created_at` time range.
- Transcript history is ordered newest-first by transcript `created_at`.
- Search results are ordered by backend relevance, with newest transcript
  `created_at` as the tiebreaker.
- Transcript collection pagination is cursor-based. The default page size is
  `50`; the maximum page size is `100`.
- Transcript, session, tag, version-history, and segment listings use the same
  collection envelope: `items` plus nullable `next_cursor`.

### Mutation Semantics

- `PATCH /api/v1/transcripts/{transcript_id}` changes transcript text only.
- A transcript edit creates a new `TranscriptVersion`; it does not rewrite or
  delete prior versions.
- `PUT /api/v1/transcripts/{transcript_id}/tags` replaces the transcript's
  entire tag set with the supplied `tag_ids`.
- Replacing tags does not create a new `TranscriptVersion`.
- Session membership is assigned only at transcription submission time in
  `v0.1.0`. No later transcript-to-session reassignment endpoint exists.

### Deletion and Retention

- Transcript retention is indefinite until explicit transcript deletion.
- Transcript deletion is a hard delete with no grace period.
- Deleting a transcript cascades to its versions, segments, current embedding,
  and tag associations.
- The originating `TranscriptionJob` survives transcript deletion as the durable
  replay record for the accepted submission attempt.
- If a deleted transcript came from a succeeded job, that job remains
  `succeeded` and its `transcript_id` becomes `null`.
- Deleting a session sets `session_id` to `null` on contained transcripts. The
  transcripts remain otherwise unchanged.
- Deleting a tag removes its transcript associations. The transcripts remain
  otherwise unchanged.
- The backend retains accepted audio only while a job is `queued`,
  `processing`, or `retry_waiting`.
- Once a job reaches `succeeded` or `failed`, the backend must promptly delete
  the retained audio.
- Failure to delete retained audio does not create a new public job state. The
  job remains terminal while cleanup is retried internally and surfaced to
  operators through logs and metrics.

### Error Contract

- Every `4xx` and `5xx` response body is JSON with at least `error_code` and
  `message`.
- `details` is optional and carries structured validation or conflict context
  when useful.
- Validation errors use `details` to identify request fields that failed
  validation.
- Error response structure is shared across job, transcript, tag, and session
  endpoints.

## Acceptance

The backend requirements are acceptable for `v0.1.0` when the following are
true:

- Valid submission returns a durable job resource instead of a blocking
  transcript response.
- Replaying a submission with the same API key, `Idempotency-Key`, and
  accepted submission tuple returns the same job in every state.
- Reusing an accepted `Idempotency-Key` with a mismatch on any accepted
  submission dimension returns `409 Conflict` and leaves the original job
  unchanged.
- Accepted jobs survive backend restart and still reach a terminal state.
- Backend-managed retries move transient failures through `retry_waiting`
  instead of forcing client resubmission.
- `retry_count` is always visible on the job resource, and `next_attempt_at`
  is visible during `retry_waiting`.
- Successful jobs create one transcript each and only completed transcripts
  appear in transcript history, detail, session transcript listings, and
  search.
- Deleting a transcript does not delete its originating job, and replay of the
  same accepted submission tuple still returns that original job.
- The transcript substrate is concrete: transcripts carry current text,
  sessions, tags, and current version identity; versions are append-only;
  segments are first-class timing rows; embeddings are stored server-side and
  regenerated asynchronously after text edits.
- Search behavior is explicit: history and search share one transcript
  collection contract, results are transcript-only, and semantic search is
  eventually consistent after edits.
- Deletion semantics are explicit and match resource ownership expectations.
- Supported audio formats, maximum upload size, language-hint semantics,
  pagination shape, and error-envelope semantics are named concretely.
- No vendor product names appear in contract shapes or response field names,
  and no implementation substrate leaks into the backend requirements.
