# Oracy Backend Requirements

Target release: `v0.1.0`

## Transcription Pipeline

### Capability

The backend accepts authenticated audio submissions, creates durable
transcription jobs, drives those jobs to a terminal state without requiring
client resubmission, and materializes successful jobs as searchable transcript
records. The public workflow is asynchronous: submission creates a job
resource, polling observes that job's progress, and transcript history/detail
surfaces only completed transcripts.

### Constraints

#### Resource Model

- A submission creates a `TranscriptionJob`, not a transcript placeholder.
- A successful job creates exactly one `Transcript`.
- Failed and in-flight jobs do not appear in transcript history, transcript
  detail, or search results.
- Transcript resources are long-lived user artifacts. Job resources are
  workflow artifacts that may point to a transcript once complete.

#### Cost Attribution

- The transcript resource includes a `cost_cents` field recording the
  backend's estimated cost to produce the transcript. The field is present on
  every transcript resource; its value is nullable.
- When the backend can compute cost from engine-provided data using
  fixed-rate pricing, `cost_cents` carries a numeric estimate.
- `cost_cents` is `null` when the cost cannot be determined without dynamic
  pricing infrastructure.

#### Durable Acceptance

- `202 Accepted` is a durability commitment, not an admission of best effort.
- A request may return `202` only after the backend has durably persisted:
  the job record, the accepted audio payload, and the accepted audio content
  hash used for idempotency matching.
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
- The backend must not advertise durable acceptance semantics when that
  storage location is unavailable or not writable.
- `spec/deployment.md` must later define the operator's persistent-storage
  responsibilities, including path provisioning, persistence across restart,
  and capacity implications.

#### Submission Contract

- Every submission requires authentication by API key.
- Every submission requires an `Idempotency-Key`, including web submissions.
- The request body contains the uploaded audio file and may include an
  optional language hint.
- `prompt` is out of scope for `v0.1.0`.
- Validation that can be performed before acceptance must remain synchronous:
  authentication, filename presence, supported format, file-size limit,
  language syntax, and idempotency-header validity.

#### Idempotency

- One submission attempt is identified by `API key + Idempotency-Key +
  accepted audio content hash`.
- The backend computes and stores the accepted audio content hash at
  acceptance time.
- Reusing the same API key and `Idempotency-Key` with the same audio content
  returns the original job instead of creating duplicate work.
- Reusing the same API key and `Idempotency-Key` with different audio content
  is a client conflict and must return `409 Conflict`.
- One `Idempotency-Key` names one attempt forever. Reusing it after terminal
  failure returns the same failed job. An intentional fresh attempt requires a
  new key.
- Idempotency isolation is per API key. Different API keys may reuse the same
  `Idempotency-Key` without colliding.

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

- A job enters `succeeded` only after its transcript record is durably stored.
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
- Transcript ordering for history is newest-first by transcript creation time.

#### Retention

- The backend retains accepted audio only while a job is `queued`,
  `processing`, or `retry_waiting`.
- Once a job reaches `succeeded` or `failed`, the backend must promptly delete
  the retained audio.
- Failure to delete retained audio does not create a new public job state. The
  job remains terminal while cleanup is retried internally and surfaced to
  operators through logs and metrics.

### Acceptance

The transcription pipeline is acceptable for `v0.1.0` when the following are
true:

- Valid submission returns a durable job resource instead of a blocking
  transcript response.
- Replaying a submission with the same API key, `Idempotency-Key`, and audio
  content returns the same job in every state.
- Reusing an accepted `Idempotency-Key` with different audio content returns
  `409 Conflict` and leaves the original job unchanged.
- Accepted jobs survive backend restart and still reach a terminal state.
- Backend-managed retries move transient failures through `retry_waiting`
  instead of forcing client resubmission.
- `retry_count` is always visible on the job resource, and `next_attempt_at`
  is visible during `retry_waiting`.
- Successful jobs create one transcript each and only completed transcripts
  appear in transcript history/detail/search.
- Retained audio is not kept beyond terminal completion of the job.
