# Oracy API Contract

Target release: `v0.1.0`

## Exigence

A person speaks. The words matter, but the speaking is fleeting. Oracy
is the system that catches voice and keeps what was said: the user
records audio, and the system produces a **voice note** — a durable
text artifact derived from that audio that the user can search, edit,
organize, and return to later. The voice note is what survives. Audio
is the input that produces it; once a voice note exists and its
embedding is stored, the audio's job on the server is done.

This contract is the wire surface between the Oracy client and the
Oracy backend. It encodes how voice notes are created (chunked audio
upload composed into a transcription job), how they are read and
mutated, how they are organized (free-text tags and optional
sessions), how they are searched (keyword, semantic, hybrid against
the current voice-note text and its current embedding), and how the
user configures the transcription engine that produces them. Polling
cadence guidance, rate limits, and long-poll versus short-poll
behavior are deferred to a later revision.

## Authentication

- All endpoints require `Authorization: Bearer <api_key>`.
- The `Bearer` auth scheme is matched case-insensitively.
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
- A request to an existing route with an unsupported method returns
  `405 Method Not Allowed` with `error_code: "method_not_allowed"`.
- JSON request body parsing preserves transport-level statuses in the
  envelope: malformed JSON returns `400 Bad Request` with
  `error_code: "malformed_json"`; unsupported JSON content type returns
  `415 Unsupported Media Type` with
  `error_code: "unsupported_content_type"`; a valid JSON body that
  cannot be deserialized into the endpoint's typed request schema
  returns `422 Unprocessable Entity` with
  `error_code: "invalid_request_shape"`; a JSON body exceeding the
  configured body limit returns `413 Payload Too Large` with
  `error_code: "payload_too_large"`.

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

## Voice Note Submission

A voice note is created through a three-step submission protocol: open
a transcription job, push audio chunks against it, finalize. Audio is
chunked client-side so each chunk fits within the per-chunk size
ceiling defined below; the server composes the chunks into the job's
input. The user-facing artifact is one voice note resulting from one
recording regardless of how many chunks the client sent.

The `Idempotency-Key` is supplied on the open call only. Subsequent
chunk and finalize calls scope idempotency through the `{job_id}` in
the path; they do not carry their own `Idempotency-Key` headers.

The supported audio formats for `v0.1.0` are `m4a`, `mp3`, `wav`, and
`webm`. The per-chunk size ceiling is `25 MiB` (`26,214,400` bytes —
the actual server-enforced limit, which OpenAI's documentation
describes loosely as "25 MB"). Both `v0.1.0` engines share this
per-call ceiling, so the per-chunk ceiling is fixed for `v0.1.0` and
does not vary by configured `transcription_model`. A future engine
with a different per-call ceiling would require an explicit contract
change. The `language` parameter, when supplied, is an ISO 639-1 hint
that constrains transcription to the supplied language and disables
engine-side auto-detection for the affected job.

### POST `/api/v1/transcription-jobs`

Open a submission attempt. No audio bytes cross this endpoint.

Request:

- Content type: `application/json`
- Headers:
  - `Idempotency-Key` required
- Body:

```json
{
  "recorded_at": "2026-04-21T18:29:55Z",
  "chunk_count": 3,
  "audio_format": "m4a",
  "session_id": "01JS9P0X3NM4Q5R6S7T8U9V0W1",
  "language": "en"
}
```

Fields:

- `recorded_at` required: RFC 3339 UTC timestamp describing when the
  audio was recorded
- `chunk_count` required: number of chunks the client will push;
  integer in `1..256`
- `audio_format` required: one of the supported audio formats
- `session_id` optional: existing session identifier owned by the
  authenticated API key
- `language` optional: ISO 639-1 language hint

Validation:

- `chunk_count` must be a positive integer not exceeding `256`.
- `session_id`, when present, must identify an existing session owned
  by the authenticated API key.

Responses:

- `201 Created`: new submission attempt opened, or replay of an
  already-open attempt under the same `(API key, Idempotency-Key)`
  with a matching open-call body
- `200 OK`: replay against an already-terminated attempt (succeeded
  or failed) under the same `(API key, Idempotency-Key)` with a
  matching open-call body
- `400 Bad Request`: invalid idempotency header, malformed JSON, or
  invalid body fields
- `401 Unauthorized`: missing or invalid API key
- `404 Not Found`: supplied `session_id` does not exist for the
  authenticated API key
- `409 Conflict`: same `(API key, Idempotency-Key)` with a mismatch on
  any open-call dimension (`recorded_at`, `chunk_count`,
  `audio_format`, `session_id`, `language`) relative to the prior
  attempt
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

Response body is a `TranscriptionJob` resource. New attempts are in
status `accepting_chunks`.

### POST `/api/v1/transcription-jobs/{job_id}/chunks`

Push one chunk of audio against an open submission attempt.

Request:

- Content type: `multipart/form-data`
- Fields:
  - `chunk_index` required: zero-based chunk position, integer in
    `0..chunk_count-1`
  - `chunk_sha256` required: lowercase hex SHA-256 of the chunk bytes
  - `file` required: the chunk's audio bytes

Validation:

- The job must exist and be in status `accepting_chunks`.
- `chunk_index` must fall within `0..chunk_count-1`.
- The chunk bytes must not exceed the per-chunk size ceiling.
- The supplied `chunk_sha256` must match the SHA-256 of the received
  chunk bytes.

Idempotency at chunk granularity: re-submitting the same
`(chunk_index, chunk_sha256)` pair against the same job is a no-op
that returns `204 No Content`. A chunk push for a `chunk_index` that
already has a different accepted hash returns `409 Conflict` and
leaves the previously accepted chunk in place.

Responses:

- `204 No Content`: chunk accepted, or idempotent replay of an
  already-accepted chunk
- `400 Bad Request`: malformed multipart, missing fields, or
  `chunk_sha256` does not match the chunk bytes
- `401 Unauthorized`
- `404 Not Found`: no such job for the authenticated API key
- `409 Conflict`: job is not in `accepting_chunks`, or the
  `chunk_index` already has a different accepted hash
- `413 Payload Too Large`: chunk bytes exceed the per-chunk ceiling

### POST `/api/v1/transcription-jobs/{job_id}/finalize`

Seal a submission attempt and commit it to durable acceptance.

Request body is empty.

The backend composes the accepted chunks (in `chunk_index` order) into
a single accepted audio content hash and persists the composed audio
artifact. Durable acceptance commits at this moment, not on the
per-chunk pushes. The job transitions `accepting_chunks → queued`.

Responses:

- `202 Accepted`: new finalization, durable acceptance committed
- `200 OK`: replay of an already-finalized job
- `400 Bad Request`: declared/observed chunk set is internally
  inconsistent
- `401 Unauthorized`
- `404 Not Found`
- `409 Conflict`: job is not in `accepting_chunks`, or one or more
  declared chunk indexes have not been accepted
- `5xx`: synchronous persistence failure during finalize; the job
  remains in `accepting_chunks` and may be retried

Response body is a `TranscriptionJob` resource.

### GET `/api/v1/transcription-jobs/{job_id}`

Fetch the current state of a transcription job.

Responses:

- `200 OK`: job found and owned by the authenticated API key
- `401 Unauthorized`
- `404 Not Found`

Response body is a `TranscriptionJob` resource.

## Voice Notes

### GET `/api/v1/voice-notes`

List voice notes for the authenticated API key. This endpoint is the
unified collection for both history and search. `v0.1.0` does not
duplicate the collection surface: history with no `q` and search with
`q` share the same shape, filters, and ordering rules.

Query parameters:

- `cursor` optional
- `limit` optional
- `q` optional: search query text
- `search_mode` optional when `q` is present: one of `keyword`,
  `semantic`, `hybrid`; defaults to `hybrid` when omitted
- `tag_id` optional and repeatable; repeated values combine by
  intersection (`AND`)
- `session_id` optional
- `recorded_after` optional: exclusive lower bound on `recorded_at`
- `recorded_before` optional: inclusive upper bound on `recorded_at`
- `created_after` optional: exclusive lower bound on `created_at`
- `created_before` optional: inclusive upper bound on `created_at`

Responses:

- `200 OK`
- `400 Bad Request`: invalid query parameter values, or `search_mode`
  supplied without `q`
- `401 Unauthorized`

Notes:

- Without `q`, the endpoint returns voice-note history ordered
  newest-first by voice-note `created_at`, with descending `id` as
  the deterministic tiebreaker for cursor pagination.
- With `q`, the endpoint returns voice-note search results using the
  same `VoiceNote` resource shape, filters, and collection envelope.
- If `q` is present and `search_mode` is omitted, the backend uses
  `hybrid`.
- Search results are always voice-note resources.
- Keyword search may match current voice-note text and historical
  `VoiceNoteVersion` text, but the returned item is always the parent
  `VoiceNote`.
- Semantic and hybrid search use the current embedding. After a
  voice-note text edit, semantic freshness is eventual rather than
  immediate.
- Search results are ordered by backend relevance score descending,
  then by voice-note `created_at` descending, then by descending `id`
  as the final deterministic tiebreaker for cursor pagination.
- Repeated `tag_id` filters combine by intersection: a voice note
  matches only if it is associated with all supplied tags.
- Failed and in-flight transcription jobs are not listed here.

Response body is a voice-note collection envelope whose `items` are
`VoiceNote` resources.

### GET `/api/v1/voice-notes/{voice_note_id}`

Fetch one voice note for the authenticated API key.

Responses:

- `200 OK`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Only completed voice notes are addressable here. A
  `TranscriptionJob` ID is never valid on this endpoint.

Response body is a `VoiceNote` resource.

### PATCH `/api/v1/voice-notes/{voice_note_id}`

Replace the current voice-note text.

Request body:

```json
{
  "text": "Updated voice note text."
}
```

Responses:

- `200 OK`
- `400 Bad Request`: malformed JSON or invalid body fields
- `401 Unauthorized`
- `404 Not Found`
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

Notes:

- The request changes voice-note text only.
- Editing is the only adjustment path for an existing voice note. The
  backend does not re-transcribe audio against an existing voice note.
- A successful edit creates one new `VoiceNoteVersion` and moves
  `VoiceNote.current_version_id` to the new version.
- Prior versions remain available through the version-history endpoint.
- Segment timing remains unchanged by text edits.
- Embedding regeneration is asynchronous. Full-text reads return the
  edited text immediately; semantic and hybrid search may lag until
  regeneration completes.

Response body is the updated `VoiceNote` resource.

### DELETE `/api/v1/voice-notes/{voice_note_id}`

Hard-delete a voice note.

Responses:

- `204 No Content`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Deletion cascades to voice-note versions, segments, the current
  embedding, and tag associations.
- The originating succeeded `TranscriptionJob` survives as the durable
  replay record for the accepted submission attempt, and its
  `voice_note_id` becomes `null`.

### GET `/api/v1/voice-notes/{voice_note_id}/versions`

List voice-note versions for one voice note.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `400 Bad Request`: malformed `cursor` or `limit` outside `1..100`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Versions are returned newest-first by `created_at`, with descending
  `id` as the deterministic tiebreaker for cursor pagination.
- Version history is linear and append-only.

Response body is a collection envelope whose `items` are
`VoiceNoteVersion` resources.

### GET `/api/v1/voice-notes/{voice_note_id}/segments`

List timing segments for one voice note.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `400 Bad Request`: malformed `cursor` or `limit` outside `1..100`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Segments are returned in ascending `position` order.
- Segment content is stable across voice-note text edits.

Response body is a collection envelope whose `items` are `Segment`
resources.

### PUT `/api/v1/voice-notes/{voice_note_id}/tags`

Replace the voice note's entire tag set.

Request body:

```json
{
  "tag_ids": ["01JS9P0Q0THR2X3E4A5B6C7D8E", "01JS9P0R6CK9M0N1P2Q3R4S5T6"]
}
```

Responses:

- `200 OK`
- `400 Bad Request`: malformed JSON or invalid body fields
- `401 Unauthorized`
- `404 Not Found`: voice note or one or more referenced tags not found
  for the authenticated API key
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

Notes:

- `PUT` replaces the voice note's entire tag set. The request body is
  the full desired set.
- Duplicate `tag_ids` are invalid.
- Replacing tags does not create a new `VoiceNoteVersion`.

Response body is the updated `VoiceNote` resource.

## Tags

### GET `/api/v1/tags`

List tags for the authenticated API key.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `400 Bad Request`: malformed `cursor` or `limit` outside `1..100`
- `401 Unauthorized`

Notes:

- Tags are returned newest-first by `created_at`, with descending `id`
  as the deterministic tiebreaker for cursor pagination.

Response body is a collection envelope whose `items` are `Tag`
resources.

### POST `/api/v1/tags`

Create a tag, or return the existing tag with the same case-insensitive
name.

Request body:

```json
{
  "name": "Meeting"
}
```

Responses:

- `201 Created`: new tag created
- `200 OK`: a tag with the same case-insensitive name already exists
  for the authenticated API key
- `400 Bad Request`: malformed JSON or invalid body fields
- `401 Unauthorized`
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

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
- `400 Bad Request`: malformed JSON or invalid body fields
- `401 Unauthorized`
- `404 Not Found`
- `409 Conflict`: another tag with the same case-insensitive name
  already exists for the authenticated API key
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

Notes:

- A successful rename updates the visible tag name on every voice note
  associated with that tag.

Response body is the updated `Tag` resource.

### DELETE `/api/v1/tags/{tag_id}`

Delete a tag.

Responses:

- `204 No Content`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Deleting a tag removes its voice-note associations but does not
  delete any voice note.

## Sessions

### GET `/api/v1/sessions`

List sessions for the authenticated API key.

Query parameters:

- `cursor` optional
- `limit` optional

Responses:

- `200 OK`
- `400 Bad Request`: malformed `cursor` or `limit` outside `1..100`
- `401 Unauthorized`

Notes:

- Sessions are returned newest-first by `created_at`, with descending
  `id` as the deterministic tiebreaker for cursor pagination.

Response body is a collection envelope whose `items` are `Session`
resources.

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
- `400 Bad Request`: malformed JSON or invalid body fields
- `401 Unauthorized`
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

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
- `400 Bad Request`: malformed JSON or invalid body fields
- `401 Unauthorized`
- `404 Not Found`
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON
- `422 Unprocessable Entity`: valid JSON does not match the typed
  request schema

Response body is the updated `Session` resource.

### DELETE `/api/v1/sessions/{session_id}`

Delete a session.

Responses:

- `204 No Content`
- `401 Unauthorized`
- `404 Not Found`

Notes:

- Deleting a session preserves contained voice notes and sets their
  `session_id` to `null`.
- Deleting a session does not invalidate previously accepted replay
  records whose stored submission tuple referenced that session.

### GET `/api/v1/sessions/{session_id}/voice-notes`

List voice notes that belong to one session.

Query parameters:

- `cursor` optional
- `limit` optional
- `q` optional
- `search_mode` optional when `q` is present: one of `keyword`,
  `semantic`, `hybrid`; defaults to `hybrid` when omitted
- `tag_id` optional and repeatable; repeated values combine by
  intersection (`AND`)
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

- This endpoint applies the same collection semantics as
  `GET /api/v1/voice-notes` but with the session scope fixed by the
  path, including the same ordering and time-bound semantics.
- If `q` is present and `search_mode` is omitted, the backend uses
  `hybrid`.
- Repeated `tag_id` filters combine by intersection.

Response body is a voice-note collection envelope whose `items` are
`VoiceNote` resources.

## Settings

The settings resource carries user-level configuration scoped to the
authenticated API key. `v0.1.0` exposes one setting:
`transcription_model`.

### GET `/api/v1/settings`

Fetch the authenticated API key's settings.

Responses:

- `200 OK`
- `401 Unauthorized`

Response body is a `Settings` resource. On first read for an API key
that has never updated settings, the response carries the documented
defaults.

### PATCH `/api/v1/settings`

Partial update of settings. Omitted fields are left unchanged.
An empty object is valid and returns the current settings unchanged.

Request body:

```json
{
  "transcription_model": "gpt-4o-transcribe"
}
```

Validation:

- `transcription_model`, when present, must be one of the supported
  transcription model identifiers documented under the engine surface.
- Field values must be of the documented type. Explicit `null` is not
  a valid value for any setting field; to leave a setting unchanged,
  omit the field.

Responses:

- `200 OK`
- `400 Bad Request`: malformed JSON, unknown field, invalid setting
  field value, or unsupported model identifier
- `401 Unauthorized`
- `413 Payload Too Large`: JSON request body exceeds the configured
  body limit
- `415 Unsupported Media Type`: request body is not sent as JSON

Notes:

- The selected `transcription_model` applies to every transcription job
  that reaches `queued` after the update. Jobs already past `queued`
  are unaffected.

Response body is the updated `Settings` resource.

### Engine Surface

The `v0.1.x` engine family is OpenAI's transcription models. `v0.1.0`
ships two model identifiers:

- `gpt-4o-mini-transcribe` — default. Fast, low-cost, accuracy
  suitable for typical voice notes.
- `gpt-4o-transcribe` — quality upgrade. Higher accuracy at higher
  cost, suitable for difficult audio (accents, noise, technical
  vocabulary).

The default `transcription_model` for an API key that has never
updated settings is `gpt-4o-mini-transcribe`.

Engine identifiers in the contract are closed-enum strings whose
internal structure carries no contract meaning. Adding or removing
engines in future releases extends or contracts the enum but does
not alter the shape of any endpoint or resource.

## Resource Schemas

### `TranscriptionJob`

```json
{
  "id": "01JS8D2PR4W8VW6TQZ0N8M1T0K",
  "status": "accepting_chunks",
  "created_at": "2026-04-21T18:30:00Z",
  "updated_at": "2026-04-21T18:30:14Z",
  "chunk_count": 3,
  "chunks_received": 1,
  "retry_count": 0,
  "max_retries": 3,
  "next_attempt_at": null,
  "failure_code": null,
  "failure_message": null,
  "retryable_by_client": null,
  "voice_note_id": null
}
```

Fields:

- `id`: job identifier
- `status`: one of `accepting_chunks`, `queued`, `processing`,
  `retry_waiting`, `succeeded`, `failed`
- `created_at`: open-call timestamp
- `updated_at`: last state-transition or chunk-acceptance timestamp
- `chunk_count`: declared chunk count from the open call
- `chunks_received`: number of distinct accepted chunks; reaches
  `chunk_count` before finalize succeeds
- `retry_count`: number of backend retry attempts already consumed;
  always returned
- `max_retries`: maximum backend retry attempts for this job
- `next_attempt_at`: returned when `status=retry_waiting`, otherwise
  omitted or `null`
- `failure_code`: nullable closed-enum string present when a failure
  classification exists. Supported values are `audio_invalid`,
  `engine_timeout`, `engine_rate_limited`, `engine_error`,
  `storage_error`, `internal_error`, and `submission_abandoned`.
  `spec/backend-requirements.md` Failure Semantics defines what each
  code means.
- `failure_message`: human-readable explanation aligned with
  `failure_code`
- `retryable_by_client`: whether a terminal failed job should be
  retried by creating a fresh submission with a new `Idempotency-Key`
- `voice_note_id`: present when `status=succeeded` and the voice note
  is still addressable; `null` for non-succeeded jobs and for
  succeeded jobs whose voice note has been deleted

Semantics:

- `chunk_count` and `chunks_received` are part of the job resource so
  the client can render upload progress and decide when to call
  `finalize`.
- `retry_count` and `next_attempt_at` are part of the job resource
  because the client uses them for UX and polling suppression.
- The same `TranscriptionJob` resource is returned for idempotent
  replays.

### `VoiceNote`

```json
{
  "id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "current_version_id": "01JS9P1D6CK9M0N1P2Q3R4S5T6",
  "text": "Hello, this is a voice note.",
  "audio_duration_seconds": 12.5,
  "audio_format": "m4a",
  "audio_size_bytes": 401280,
  "language": "en",
  "model": "gpt-4o-mini-transcribe",
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

- `id`: voice-note identifier
- `current_version_id`: identifier of the current voice-note version
- `text`: current voice-note text
- `audio_duration_seconds`: composed audio duration
- `audio_format`: accepted audio format
- `audio_size_bytes`: composed audio size in bytes
- `language`: the language hint or detected language for the
  transcription
- `model`: transcription engine identifier in use when the voice note
  was produced
- `processing_time_ms`: total server-side processing time
- `cost_cents`: backend cost estimate in cents; always present and
  nullable
- `created_at`: voice-note creation timestamp
- `recorded_at`: client-supplied audio capture timestamp
- `session_id`: associated session identifier, or `null`
- `tags`: associated `Tag` resources

Semantics:

- `audio_duration_seconds`, `audio_format`, and `audio_size_bytes` are
  durable provenance properties of the voice note. The composed audio
  bytes are released from the server once the originating job reaches
  a terminal state; these fields persist regardless.

### `VoiceNoteVersion`

```json
{
  "id": "01JS9P1D6CK9M0N1P2Q3R4S5T6",
  "voice_note_id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "text": "Hello, this is a voice note.",
  "created_at": "2026-04-21T18:31:19Z"
}
```

Fields:

- `id`: voice-note-version identifier
- `voice_note_id`: parent voice-note identifier
- `text`: voice-note text captured in this version
- `created_at`: version creation timestamp

### `Segment`

```json
{
  "id": "01JS9P1K2AQ3B4C5D6E7F8G9H0",
  "voice_note_id": "01JS8D6E2S3T1J7H9J2Q2N4P5R",
  "position": 0,
  "start_ms": 0,
  "end_ms": 1480,
  "text": "Hello, this is a voice note."
}
```

Fields:

- `id`: segment identifier
- `voice_note_id`: parent voice-note identifier
- `position`: zero-based segment order
- `start_ms`: segment start offset in milliseconds, measured from the
  start of the composed audio
- `end_ms`: segment end offset in milliseconds, measured from the
  start of the composed audio
- `text`: segment text captured from the transcription result

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

### `Settings`

```json
{
  "transcription_model": "gpt-4o-mini-transcribe"
}
```

Fields:

- `transcription_model`: the engine identifier used for every
  transcription job that reaches `queued` after the most recent update

## Idempotency Rules

- A submission attempt is identified by
  `API key + Idempotency-Key + accepted audio content hash +
  recorded_at + session_id + language`.
- The `Idempotency-Key` is supplied on the open call
  (`POST /api/v1/transcription-jobs`) and governs replay matching for
  the entire submission attempt.
- The accepted audio content hash is computed at finalize time as a
  hash composed deterministically from the accepted chunk hashes in
  `chunk_index` order.
- The accepted submission tuple is an immutable acceptance-time
  record, not a live reference to current resource state.
- The original open-call body's `chunk_count` and `audio_format` are
  durably stored alongside the accepted submission tuple for the
  lifetime of the replay record so that terminal-replay matching
  against a fresh open call survives backend restart.
- For `recorded_at`, replay matching compares the parsed instant
  normalized to UTC, not the raw wire-format string. Any RFC 3339 UTC
  representation of the same instant is a match.
- Omitted optional `session_id` and `language` values participate as
  `null`.
- Same API key + same `Idempotency-Key` + same accepted submission
  tuple returns the same `TranscriptionJob`.
- Same API key + same `Idempotency-Key` + a mismatch on any open-call
  dimension (before finalize) or any accepted submission dimension
  (after finalize) returns `409 Conflict`.
- For a new open call against an already-terminated attempt
  (succeeded or failed) under the same `(API key, Idempotency-Key)`,
  replay matching compares the new open-call body's `recorded_at`,
  `chunk_count`, `audio_format`, `session_id`, and `language` against
  the original open-call values. All match returns the original
  terminated `TranscriptionJob` with `200 OK`. Any mismatch returns
  `409 Conflict`. The accepted audio content hash does not
  participate in this comparison because a fresh open call carries no
  audio bytes.
- Chunk pushes are idempotent on `(chunk_index, chunk_sha256)`.
- Idempotency replay takes precedence over session-existence
  validation on the open call. When a replay match exists under the
  same `(API key, Idempotency-Key)`, the backend returns the original
  `TranscriptionJob` even if the supplied `session_id` no longer
  identifies an existing session for the authenticated API key.
- If the session referenced by an accepted `session_id` is later
  deleted, idempotent replays still return the original
  `TranscriptionJob` matched by the stored accepted tuple.
- If the voice note created by a succeeded job is later deleted,
  idempotent replays still return the original succeeded
  `TranscriptionJob` with `voice_note_id=null`.
- An intentional fresh submission attempt after terminal failure
  must use a new `Idempotency-Key`. Reusing the original key returns
  the same terminated job rather than starting a new attempt.

## Contract Notes

- This contract intentionally omits a synchronous voice-note-returning
  upload endpoint.
- This contract intentionally omits `prompt` from `v0.1.0`.
- This contract intentionally omits public API-key management
  endpoints.
- This contract intentionally omits a runtime engine-discovery
  endpoint; the supported transcription model identifiers are
  enumerated in this document.
- This contract intentionally omits embedding-vector export.
- Speaker diarization, branching edit history, and voice-note-to-
  session reassignment after submission are deferred beyond `v0.1.0`.
- Polling cadence, rate limits, and backoff advice are deferred to a
  later API contract revision.
