# Oracy API Contract

Target release: `v0.1.0`

This document captures the public HTTP contract required by the coordinated
Flutter client and Rust backend. Polling cadence guidance, rate limits, and
long-poll versus short-poll behavior are deferred to a later revision.

## Authentication

- All endpoints below require `Authorization: Bearer <api_key>`.
- API keys are provisioned out-of-band by the operator.
- Idempotency is scoped per authenticated API key.
- Every resource lookup is scoped to the authenticated API key.
- `v0.1.0` exposes no public endpoint for API-key creation, rotation,
  revocation, or deletion.

## Shared Conventions

- Timestamps use RFC 3339 UTC strings.
- Resource IDs are opaque strings.
- Collection endpoints return the same envelope shape:

```json
{
  "items": [],
  "next_cursor": null
}
```

- `next_cursor` is `null` when no additional page exists.
- `limit` defaults to `50` and must not exceed `100`.
- All `4xx` and `5xx` responses use the same `ErrorResponse` envelope.

### `ErrorResponse`

```json
{
  "error_code": "validation_error",
  "message": "One or more request fields are invalid.",
  "details": [
    {
      "field": "language",
      "message": "Must be a valid ISO 639-1 code."
    }
  ]
}
```

Fields:

- `error_code`: stable machine-readable error identifier
- `message`: human-readable summary
- `details`: optional structured context for validation or conflict errors

## Endpoint Inventory

### POST `/api/v1/transcriptions`

Submit audio for asynchronous transcription.

Request:

- Content type: `multipart/form-data`
- Fields:
  - `file` required: uploaded audio file
  - `recorded_at` required: RFC 3339 UTC timestamp describing when the audio
    was recorded
  - `session_id` optional: existing session identifier owned by the
    authenticated API key
  - `language` optional: ISO 639-1 language hint
- Headers:
  - `Idempotency-Key` required

Validation:

- Accepted file formats are `m4a`, `mp3`, `wav`, and `webm`.
- Maximum upload size is `25 MiB`.
- `session_id`, when present on a new submission attempt, must identify an
  existing session owned by the authenticated API key.
- `language`, when present, constrains transcription to the supplied language.
  The backend does not auto-detect language for that submission.

Responses:

- `202 Accepted`: new durable job accepted, or replay of an existing
  nonterminal job
- `200 OK`: replay of an existing terminal job
- `400 Bad Request`: invalid idempotency header or invalid request fields
- `401 Unauthorized`: missing or invalid API key
- `404 Not Found`: on a new submission attempt, supplied `session_id` does not
  exist for the authenticated API key
- `413 Payload Too Large`: file exceeds allowed size
- `415 Unsupported Media Type`: unsupported audio format
- `409 Conflict`: same API key and `Idempotency-Key`, but a mismatch on audio
  content, `recorded_at`, `session_id`, or `language` relative to the accepted
  submission
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

Query parameters:

- `cursor` optional
- `limit` optional
- `q` optional: search query text
- `search_mode` optional when `q` is present: one of `keyword`, `semantic`,
  `hybrid`; defaults to `hybrid` when omitted
- `tag_id` optional and repeatable; repeated values combine by intersection
  (`AND`)
- `session_id` optional
- `recorded_after` optional
- `recorded_before` optional
- `created_after` optional
- `created_before` optional

Responses:

- `200 OK`
- `400 Bad Request`: invalid query parameter values, or `search_mode` supplied
  without `q`
- `401 Unauthorized`

Notes:

- This endpoint is the unified transcript collection for both history and
  search.
- Without `q`, the endpoint returns transcript history ordered newest-first by
  transcript `created_at`.
- With `q`, the endpoint returns transcript search results using the same
  `Transcript` resource shape, filters, and collection envelope. This is an
  explicit `v0.1.0` governance decision to avoid duplicating transcript
  collection surface with no semantic gain.
- If `q` is present and `search_mode` is omitted, the backend uses `hybrid`.
- Search results are always transcript resources.
- Keyword search may match current transcript text and historical
  `TranscriptVersion` text, but the returned item is always the parent
  `Transcript`.
- Semantic and hybrid search use the current embedding. After a transcript
  text edit, semantic freshness is eventual rather than immediate.
- Repeated `tag_id` filters combine by intersection: a transcript matches only
  if it is associated with all supplied tags.
- Failed jobs and in-flight jobs are not listed.

Response body is a transcript collection envelope whose `items` are
`Transcript` resources.

### GET `/api/v1/transcripts/{transcript_id}`

Fetch one completed transcript for the authenticated API key.

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Only completed transcripts are addressable here.
- A job ID is never valid on this endpoint.

Response body is a `Transcript` resource.

### PATCH `/api/v1/transcripts/{transcript_id}`

Replace the current transcript text.

Request body:

```json
{
  "transcript": "Updated transcript text."
}
```

Responses:

- `200 OK`
- `400 Bad Request`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- The request changes transcript text only.
- A successful edit creates one new `TranscriptVersion` and moves
  `Transcript.current_version_id` to the new version.
- Prior versions remain available through the version-history endpoint.
- Segment timing remains unchanged by text edits.
- Embedding regeneration is asynchronous. Full-text reads return the edited
  text immediately; semantic and hybrid search may lag until regeneration
  completes.

Response body is the updated `Transcript` resource.

### DELETE `/api/v1/transcripts/{transcript_id}`

Hard-delete a transcript.

Responses:

- `204 No Content`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Deletion cascades to transcript versions, segments, the current embedding,
  and tag associations.
- The originating succeeded `TranscriptionJob` survives as the durable replay
  record for the accepted submission attempt, and its `transcript_id` becomes
  `null`.

### GET `/api/v1/transcripts/{transcript_id}/versions`

List transcript versions for one transcript.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Versions are returned newest-first by `created_at`.
- Version history is linear and append-only.

Response body is a collection envelope whose `items` are
`TranscriptVersion` resources.

### GET `/api/v1/transcripts/{transcript_id}/segments`

List timing segments for one transcript.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Segments are returned in ascending `position` order.
- Segment content is stable across transcript text edits.

Response body is a collection envelope whose `items` are `Segment` resources.

### PUT `/api/v1/transcripts/{transcript_id}/tags`

Replace the transcript's entire tag set.

Request body:

```json
{
  "tag_ids": ["01JS9P0Q0THR2X3E4A5B6C7D8E", "01JS9P0R6CK9M0N1P2Q3R4S5T6"]
}
```

Responses:

- `200 OK`
- `400 Bad Request`
- `401 Unauthorized`
- `404 Not Found`: transcript or one or more referenced tags not found for the
  authenticated API key

Notes:

- `PUT` replaces the transcript's entire tag set. The request body is the full
  desired set.
- Duplicate `tag_ids` are invalid.
- Replacing tags does not create a new `TranscriptVersion`.

Response body is the updated `Transcript` resource.

### GET `/api/v1/tags`

List tags for the authenticated API key.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `401 Unauthorized`

Response body is a collection envelope whose `items` are `Tag` resources.

### POST `/api/v1/tags`

Create a tag, or return the existing tag with the same case-insensitive name.

Request body:

```json
{
  "name": "Meeting"
}
```

Responses:

- `201 Created`: new tag created
- `200 OK`: a tag with the same case-insensitive name already exists for the
  authenticated API key
- `400 Bad Request`
- `401 Unauthorized`

Notes:

- Tag identity is case-insensitive within one API-key scope.
- The returned `Tag.name` preserves the stored spelling.

Response body is a `Tag` resource.

### GET `/api/v1/tags/{tag_id}`

Fetch one tag.

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Response body is a `Tag` resource.

### PATCH `/api/v1/tags/{tag_id}`

Rename a tag.

Request body:

```json
{
  "name": "Planning"
}
```

Responses:

- `200 OK`
- `400 Bad Request`
- `401 Unauthorized`
- `404 Not Found`
- `409 Conflict`: another tag with the same case-insensitive name already
  exists for the authenticated API key

Notes:

- A successful rename updates the visible tag name on every transcript
  associated with that tag.

Response body is the updated `Tag` resource.

### DELETE `/api/v1/tags/{tag_id}`

Delete a tag.

Responses:

- `204 No Content`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Deleting a tag removes its transcript associations but does not delete any
  transcript.

### GET `/api/v1/sessions`

List sessions for the authenticated API key.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `401 Unauthorized`

Response body is a collection envelope whose `items` are `Session` resources.

### POST `/api/v1/sessions`

Create a session.

Request body:

```json
{
  "name": "Q2 Planning"
}
```

Responses:

- `201 Created`
- `400 Bad Request`
- `401 Unauthorized`

Response body is a `Session` resource.

### GET `/api/v1/sessions/{session_id}`

Fetch one session.

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Response body is a `Session` resource.

### PATCH `/api/v1/sessions/{session_id}`

Rename a session.

Request body:

```json
{
  "name": "Customer Interviews"
}
```

Responses:

- `200 OK`
- `400 Bad Request`
- `401 Unauthorized`
- `404 Not Found`

Response body is the updated `Session` resource.

### DELETE `/api/v1/sessions/{session_id}`

Delete a session.

Responses:

- `204 No Content`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Deleting a session preserves contained transcripts and sets their
  `session_id` to `null`.
- Deleting a session does not invalidate previously accepted replay records
  whose stored submission tuple referenced that session.

### GET `/api/v1/sessions/{session_id}/transcripts`

List completed transcripts that belong to one session.

Query parameters:

- `cursor` optional
- `limit` optional
- `q` optional
- `search_mode` optional when `q` is present: one of `keyword`, `semantic`,
  `hybrid`; defaults to `hybrid` when omitted
- `tag_id` optional and repeatable; repeated values combine by intersection
  (`AND`)
- `recorded_after` optional
- `recorded_before` optional
- `created_after` optional
- `created_before` optional

Responses:

- `200 OK`
- `400 Bad Request`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- This endpoint applies the same collection semantics as `GET /api/v1/transcripts`
  but with the session scope fixed by the path.
- If `q` is present and `search_mode` is omitted, the backend uses `hybrid`.
- Repeated `tag_id` filters combine by intersection: a transcript matches only
  if it is associated with all supplied tags.

Response body is a transcript collection envelope whose `items` are
`Transcript` resources.

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
  "failure_message": "The transcription attempt timed out while processing audio.",
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
- `transcript_id`: present when `status=succeeded` and the transcript is still
  addressable; `null` for non-succeeded jobs and for succeeded jobs whose
  transcript has been deleted

Semantics:

- `retry_count` and `next_attempt_at` are part of the job resource because the
  client uses them for UX and polling suppression.
- The same `TranscriptionJob` resource is returned for idempotent replays.

### `Tag`

```json
{
  "id": "01JS9P0Q0THR2X3E4A5B6C7D8E",
  "name": "Meeting",
  "created_at": "2026-04-21T18:31:30Z"
}
```

Fields:

- `id`: tag identifier
- `name`: tag name, unique case-insensitively within one API-key scope
- `created_at`: tag creation timestamp

### `Session`

```json
{
  "id": "01JS9P0X3NM4Q5R6S7T8U9V0W1",
  "name": "Q2 Planning",
  "created_at": "2026-04-21T18:31:35Z"
}
```

Fields:

- `id`: session identifier
- `name`: session display name
- `created_at`: session creation timestamp

### `Transcript`

```json
{
  "id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "current_version_id": "01JS9P1D6CK9M0N1P2Q3R4S5T6",
  "transcript": "Hello, this is a test recording.",
  "audio_duration_seconds": 12.5,
  "audio_format": "wav",
  "audio_size_bytes": 401280,
  "transcript_language": "en",
  "model": "general-transcription-v1",
  "processing_time_ms": 1843,
  "cost_cents": 1,
  "created_at": "2026-04-21T18:31:19Z",
  "recorded_at": "2026-04-21T18:29:55Z",
  "session_id": "01JS9P0X3NM4Q5R6S7T8U9V0W1",
  "tags": [
    {
      "id": "01JS9P0Q0THR2X3E4A5B6C7D8E",
      "name": "Meeting",
      "created_at": "2026-04-21T18:31:30Z"
    }
  ]
}
```

Fields:

- `id`: transcript identifier
- `current_version_id`: identifier of the current transcript version
- `transcript`: current transcript text
- `audio_duration_seconds`: source audio duration
- `audio_format`: accepted audio format
- `audio_size_bytes`: original uploaded size
- `transcript_language`: detected or hinted language
- `model`: transcription engine identifier
- `processing_time_ms`: total server-side processing time
- `cost_cents`: backend cost estimate in cents; always present and nullable
- `created_at`: transcript creation timestamp
- `recorded_at`: client-supplied audio capture timestamp
- `session_id`: associated session identifier, or `null`
- `tags`: associated `Tag` resources

### `TranscriptVersion`

```json
{
  "id": "01JS9P1D6CK9M0N1P2Q3R4S5T6",
  "transcript_id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "transcript": "Hello, this is a test recording.",
  "created_at": "2026-04-21T18:31:19Z"
}
```

Fields:

- `id`: transcript-version identifier
- `transcript_id`: parent transcript identifier
- `transcript`: transcript text captured in this version
- `created_at`: version creation timestamp

### `Segment`

```json
{
  "id": "01JS9P1K2AQ3B4C5D6E7F8G9H0",
  "transcript_id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "position": 0,
  "start_ms": 0,
  "end_ms": 1480,
  "text": "Hello, this is a test recording."
}
```

Fields:

- `id`: segment identifier
- `transcript_id`: parent transcript identifier
- `position`: zero-based segment order
- `start_ms`: segment start offset in milliseconds
- `end_ms`: segment end offset in milliseconds
- `text`: segment text captured from the transcription result

## Idempotency Rules

- The backend matches a replay by `API key + Idempotency-Key + accepted audio
content hash + recorded_at + session_id + language`.
- The accepted submission tuple is an immutable acceptance-time record, not a
  live reference to current resource state.
- Omitted optional `session_id` and `language` values participate as `null`.
- Same API key + same `Idempotency-Key` + same accepted submission tuple
  returns the same `TranscriptionJob`.
- Same API key + same `Idempotency-Key` + a mismatch on any accepted
  submission dimension returns `409 Conflict`.
- If the session referenced by an accepted `session_id` is later deleted,
  idempotent replays still return the original `TranscriptionJob` matched by
  the stored accepted tuple.
- If the transcript created by a succeeded job is later deleted, idempotent
  replays still return the original succeeded `TranscriptionJob` with
  `transcript_id=null`.
- A new submission attempt after terminal failure must use a new
  `Idempotency-Key`.

## Contract Notes

- This contract intentionally omits a synchronous transcript-returning upload
  endpoint.
- This contract intentionally omits `prompt` from `v0.1.0`.
- This contract intentionally omits public API-key management endpoints.
- Speaker diarization, branching edit history, transcript-to-session
  reassignment after submission, and embedding export are deferred beyond
  `v0.1.0`.
- Polling cadence, rate limits, and backoff advice are deferred to a later API
  contract revision.
