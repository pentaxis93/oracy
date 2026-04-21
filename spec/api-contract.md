# Oracy API Contract

Target release: `v0.1.0`

This document captures the public HTTP contract required by the coordinated
Flutter client and Rust backend. Polling cadence guidance, rate limits, and
long-poll versus short-poll behavior are deferred to a later revision.

## Authentication

- All endpoints below require `Authorization: Bearer <api_key>`.
- Idempotency is scoped per authenticated API key.

## Endpoint Inventory

### POST `/api/v1/transcriptions`

Submit audio for asynchronous transcription.

Request:

- Content type: `multipart/form-data`
- Fields:
  - `file` required: uploaded audio file
  - `language` optional: language hint
- Headers:
  - `Idempotency-Key` required

Responses:

- `202 Accepted`: new durable job accepted, or replay of an existing
  nonterminal job
- `200 OK`: replay of an existing terminal job
- `400 Bad Request`: invalid idempotency header or invalid request fields
- `401 Unauthorized`: missing or invalid API key
- `413 Payload Too Large`: file exceeds allowed size
- `415 Unsupported Media Type`: unsupported audio format
- `409 Conflict`: same API key and `Idempotency-Key`, but different audio
  content than the accepted submission
- `5xx`: synchronous failure before durable acceptance

Response body for `200` and `202` is a `TranscriptionJob` resource.

### GET `/api/v1/transcriptions/{job_id}`

Fetch the current state of a transcription job.

Responses:

- `200 OK`: job found and owned by the authenticated API key
- `401 Unauthorized`: missing or invalid API key
- `404 Not Found`: no such job for the authenticated API key

Response body is a `TranscriptionJob` resource.

### GET `/api/v1/transcripts`

List completed transcripts for the authenticated API key.

Responses:

- `200 OK`
- `401 Unauthorized`

Notes:

- Only completed transcripts appear here.
- Failed jobs and in-flight jobs are not listed.

### GET `/api/v1/transcripts/{transcript_id}`

Fetch one completed transcript for the authenticated API key.

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Only completed transcripts are addressable here.
- A job ID is never valid on this endpoint.

## Resource Schemas

### `TranscriptionJob`

```json
{
  "id": "01JS8D2PR4W8VW6TQZ0N8M1T0K",
  "status": "retry_waiting",
  "created_at": "2026-04-21T18:30:00Z",
  "updated_at": "2026-04-21T18:31:12Z",
  "retry_count": 1,
  "max_retries": 3,
  "next_attempt_at": "2026-04-21T18:32:12Z",
  "failure_code": "engine_timeout",
  "failure_message": "The transcription engine timed out while processing audio.",
  "retryable_by_client": true,
  "transcript_id": null
}
```

Fields:

- `id`: job identifier
- `status`: one of `queued`, `processing`, `retry_waiting`, `succeeded`,
  `failed`
- `created_at`: acceptance timestamp
- `updated_at`: last state-transition timestamp
- `retry_count`: number of backend retry attempts already consumed; always
  returned
- `max_retries`: maximum backend retry attempts for this job
- `next_attempt_at`: returned when `status=retry_waiting`, otherwise omitted or
  `null`
- `failure_code`: present when a failure classification exists
- `failure_message`: human-readable explanation aligned with `failure_code`
- `retryable_by_client`: whether a terminal failed job should be retried by
  creating a fresh submission with a new `Idempotency-Key`
- `transcript_id`: present when `status=succeeded`

Semantics:

- `retry_count` and `next_attempt_at` are part of the job resource because the
  client uses them for UX and polling suppression.
- The same `TranscriptionJob` resource is returned for idempotent replays.

### `Transcript`

```json
{
  "id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "transcript": "Hello, this is a test recording.",
  "audio_duration_seconds": 12.5,
  "audio_format": "wav",
  "audio_size_bytes": 401280,
  "transcript_language": "en",
  "whisper_model": "gpt-4o-transcribe",
  "processing_time_ms": 1843,
  "cost_cents": 1,
  "created_at": "2026-04-21T18:31:19Z"
}
```

Fields:

- `id`: transcript identifier
- `transcript`: transcribed text
- `audio_duration_seconds`: source audio duration
- `audio_format`: accepted audio format
- `audio_size_bytes`: original uploaded size
- `transcript_language`: detected or hinted language
- `whisper_model`: engine/model used
- `processing_time_ms`: total server-side processing time
- `cost_cents`: transcription cost tracked by the backend
- `created_at`: transcript creation timestamp

## Idempotency Rules

- The backend matches a replay by `API key + Idempotency-Key + accepted audio
  content hash`.
- Same API key + same `Idempotency-Key` + same audio content returns the same
  `TranscriptionJob`.
- Same API key + same `Idempotency-Key` + different audio content returns
  `409 Conflict`.
- A new submission attempt after terminal failure must use a new
  `Idempotency-Key`.

## Contract Notes

- This contract intentionally omits a synchronous transcript-returning upload
  endpoint.
- This contract intentionally omits `prompt` from `v0.1.0`.
- Polling cadence, rate limits, and backoff advice are deferred to a later API
  contract revision.
