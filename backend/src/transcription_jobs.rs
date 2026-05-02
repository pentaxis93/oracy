use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::audio_hash::{
    compose_audio_content_hash_hex, sha256_hex, validate_lowercase_sha256_hex,
};
use crate::audio_store::{
    MAX_CHUNK_BYTES, MULTIPART_BODY_LIMIT_BYTES, compose_chunks, persist_chunk,
};
use crate::auth::AuthenticatedKey;
use crate::collections::{parse_rfc3339_field, timestamp, validate_ulid_field, validation_error};
use crate::errors::ApiError;
use crate::json::JsonBody;
use crate::state::AppState;
use crate::storage::{
    AcceptedChunk, ChunkRecord, FinalizeJobOutcome, NewOpenTranscriptionJob, OpenJobOutcome,
    StoreChunkOutcome, TranscriptionJobRecord,
};

const MAX_RETRIES: i64 = 3;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenTranscriptionJobRequest {
    recorded_at: String,
    chunk_count: i64,
    audio_format: String,
    session_id: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptionJobResource {
    id: String,
    status: String,
    created_at: String,
    updated_at: String,
    chunk_count: i64,
    chunks_received: i64,
    retry_count: i64,
    max_retries: i64,
    next_attempt_at: Option<String>,
    failure_code: Option<String>,
    failure_message: Option<String>,
    retryable_by_client: Option<bool>,
    voice_note_id: Option<String>,
}

#[derive(Debug)]
struct ParsedChunk {
    chunk_index: i64,
    chunk_sha256: String,
    bytes: Vec<u8>,
}

struct MultipartBody(Multipart);

impl<S> FromRequest<S> for MultipartBody
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        Multipart::from_request(req, state)
            .await
            .map(Self)
            .map_err(map_multipart_rejection)
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(open_transcription_job))
        .route("/{job_id}", axum::routing::get(get_transcription_job))
        .route(
            "/{job_id}/chunks",
            post(push_chunk).layer(DefaultBodyLimit::max(MULTIPART_BODY_LIMIT_BYTES)),
        )
        .route("/{job_id}/finalize", post(finalize_transcription_job))
}

async fn open_transcription_job(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    headers: HeaderMap,
    JsonBody(body): JsonBody<OpenTranscriptionJobRequest>,
) -> Result<(StatusCode, Json<TranscriptionJobResource>), ApiError> {
    let idempotency_key = idempotency_key(&headers)?;
    let recorded_at = parse_rfc3339_field("recorded_at", &body.recorded_at)?;
    validate_open_body(&body)?;
    let outcome = state
        .storage
        .open_job(NewOpenTranscriptionJob {
            api_key_id: authenticated_key.api_key_id.to_string(),
            idempotency_key,
            recorded_at,
            session_id: body.session_id,
            language: body.language,
            chunk_count: body.chunk_count,
            audio_format: body.audio_format,
            max_retries: MAX_RETRIES,
            now: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to open transcription job."))?;

    match outcome {
        OpenJobOutcome::Created(job) | OpenJobOutcome::ReplayedOpen(job) => {
            Ok((StatusCode::CREATED, Json(job_resource(job)?)))
        }
        OpenJobOutcome::ReplayedFinalized(job) => Ok((StatusCode::OK, Json(job_resource(job)?))),
        OpenJobOutcome::Conflict(_) => Err(ApiError::conflict(
            "Idempotency key was already used with different submission fields.",
            None,
        )),
        OpenJobOutcome::SessionNotFound => Err(ApiError::not_found("Session not found.")),
    }
}

async fn push_chunk(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    MultipartBody(multipart): MultipartBody,
) -> Result<StatusCode, ApiError> {
    validate_ulid_field("job_id", &job_id)?;
    let api_key_id = authenticated_key.api_key_id.to_string();
    let Some(job) = state
        .storage
        .get_job(&api_key_id, &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load transcription job."))?
    else {
        return Err(ApiError::not_found("Transcription job not found."));
    };
    if job.status != "accepting_chunks" {
        return Err(ApiError::conflict(
            "Transcription job is not accepting chunks.",
            None,
        ));
    }

    let parsed = parse_chunk_multipart(multipart).await?;
    if parsed.chunk_index < 0 || parsed.chunk_index >= job.chunk_count {
        return Err(validation_error(
            "chunk_index",
            "Must be within the declared chunk range.",
        ));
    }
    if parsed.bytes.len() > MAX_CHUNK_BYTES {
        return Err(ApiError::payload_too_large());
    }
    let actual_sha256 = sha256_hex(&parsed.bytes);
    if actual_sha256 != parsed.chunk_sha256 {
        return Err(validation_error(
            "chunk_sha256",
            "Must match the received chunk bytes.",
        ));
    }

    if let Some(existing) = state
        .storage
        .get_chunk(&api_key_id, &job_id, parsed.chunk_index)
        .await
        .map_err(|_| ApiError::internal("Failed to load chunk state."))?
    {
        if existing.chunk_sha256_hex == parsed.chunk_sha256 {
            return Ok(StatusCode::NO_CONTENT);
        }
        return Err(ApiError::conflict(
            "Chunk index already has different accepted bytes.",
            None,
        ));
    }

    let chunk_path = persist_chunk(
        &state.accepted_audio_dir,
        &job_id,
        parsed.chunk_index,
        &parsed.bytes,
    )
    .await
    .map_err(|_| ApiError::internal("Failed to persist chunk."))?;
    let outcome = state
        .storage
        .store_chunk(AcceptedChunk {
            api_key_id,
            job_id,
            chunk_index: parsed.chunk_index,
            chunk_sha256_hex: parsed.chunk_sha256,
            chunk_path,
            chunk_size_bytes: parsed.bytes.len() as i64,
            accepted_at: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to persist chunk state."))?;

    match outcome {
        StoreChunkOutcome::Stored | StoreChunkOutcome::Replayed => Ok(StatusCode::NO_CONTENT),
        StoreChunkOutcome::Conflict => Err(ApiError::conflict(
            "Chunk index already has different accepted bytes.",
            None,
        )),
        StoreChunkOutcome::NotFound => Err(ApiError::not_found("Transcription job not found.")),
        StoreChunkOutcome::NotAcceptingChunks => Err(ApiError::conflict(
            "Transcription job is not accepting chunks.",
            None,
        )),
    }
}

async fn finalize_transcription_job(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<(StatusCode, Json<TranscriptionJobResource>), ApiError> {
    validate_ulid_field("job_id", &job_id)?;
    let api_key_id = authenticated_key.api_key_id.to_string();
    let Some((job, chunks)) = state
        .storage
        .list_chunks_for_finalize(&api_key_id, &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load transcription job."))?
    else {
        return Err(ApiError::not_found("Transcription job not found."));
    };
    if job.status != "accepting_chunks" {
        if !job.audio_sha256_hex.is_empty() {
            return Ok((StatusCode::OK, Json(job_resource(job)?)));
        }
        return Err(ApiError::conflict(
            "Transcription job is not accepting chunks.",
            None,
        ));
    }
    ensure_complete_chunk_set(&job, &chunks)?;

    let audio_sha256_hex =
        compose_audio_content_hash_hex(chunks.iter().map(|chunk| chunk.chunk_sha256_hex.as_str()))
            .map_err(|_| ApiError::internal("Accepted chunk hash state is invalid."))?;
    let accepted_audio_path = compose_chunks(
        &state.accepted_audio_dir,
        &job_id,
        &job.audio_format,
        &chunks,
    )
    .await
    .map_err(|_| ApiError::internal("Failed to compose accepted audio."))?;
    let settings = state
        .storage
        .get_settings(&api_key_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load settings."))?;

    match state
        .storage
        .finalize_job(
            &api_key_id,
            &job_id,
            &audio_sha256_hex,
            &accepted_audio_path,
            &settings.transcription_model,
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(|_| ApiError::internal("Failed to finalize transcription job."))?
    {
        FinalizeJobOutcome::Accepted(job) => Ok((StatusCode::ACCEPTED, Json(job_resource(job)?))),
        FinalizeJobOutcome::Replayed(job) => Ok((StatusCode::OK, Json(job_resource(job)?))),
        FinalizeJobOutcome::MissingChunks => Err(ApiError::conflict(
            "Every declared chunk must be accepted before finalize.",
            None,
        )),
        FinalizeJobOutcome::NotFound => Err(ApiError::not_found("Transcription job not found.")),
        FinalizeJobOutcome::NotAcceptingChunks => Err(ApiError::conflict(
            "Transcription job is not accepting chunks.",
            None,
        )),
    }
}

async fn get_transcription_job(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<TranscriptionJobResource>, ApiError> {
    validate_ulid_field("job_id", &job_id)?;
    let Some(job) = state
        .storage
        .get_job(authenticated_key.api_key_id.as_str(), &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load transcription job."))?
    else {
        return Err(ApiError::not_found("Transcription job not found."));
    };

    Ok(Json(job_resource(job)?))
}

fn validate_open_body(body: &OpenTranscriptionJobRequest) -> Result<(), ApiError> {
    if !(1..=256).contains(&body.chunk_count) {
        return Err(validation_error(
            "chunk_count",
            "Must be an integer in 1..256.",
        ));
    }
    if !matches!(body.audio_format.as_str(), "m4a" | "mp3" | "wav" | "webm") {
        return Err(validation_error(
            "audio_format",
            "Must be a supported audio format.",
        ));
    }
    if let Some(session_id) = body.session_id.as_deref() {
        validate_ulid_field("session_id", session_id)?;
    }
    if let Some(language) = body.language.as_deref()
        && (language.len() != 2 || !language.bytes().all(|byte| byte.is_ascii_lowercase()))
    {
        return Err(validation_error(
            "language",
            "Must be an ISO 639-1 language code.",
        ));
    }

    Ok(())
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let mut values = headers.get_all("Idempotency-Key").iter();
    let Some(value) = values.next() else {
        return Err(validation_error("Idempotency-Key", "Header is required."));
    };
    if values.next().is_some() {
        return Err(validation_error("Idempotency-Key", "Header is malformed."));
    }
    let value = value
        .to_str()
        .map_err(|_| validation_error("Idempotency-Key", "Header is malformed."))?;
    if value.trim().is_empty()
        || value != value.trim()
        || value.len() > 255
        || !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(validation_error("Idempotency-Key", "Header is malformed."));
    }

    Ok(value.to_owned())
}

async fn parse_chunk_multipart(mut multipart: Multipart) -> Result<ParsedChunk, ApiError> {
    let mut chunk_index = None;
    let mut chunk_sha256 = None;
    let mut file = None;

    while let Some(field) = multipart.next_field().await.map_err(map_multipart_error)? {
        let Some(name) = field.name().map(str::to_owned) else {
            continue;
        };
        match name.as_str() {
            "chunk_index" => {
                if chunk_index.is_some() {
                    return Err(validation_error(
                        "chunk_index",
                        "Must be supplied exactly once.",
                    ));
                }
                let value = field.bytes().await.map_err(map_multipart_error)?;
                let value = std::str::from_utf8(&value)
                    .map_err(|_| validation_error("chunk_index", "Must be an integer."))?;
                chunk_index = Some(
                    value
                        .parse::<i64>()
                        .map_err(|_| validation_error("chunk_index", "Must be an integer."))?,
                );
            }
            "chunk_sha256" => {
                if chunk_sha256.is_some() {
                    return Err(validation_error(
                        "chunk_sha256",
                        "Must be supplied exactly once.",
                    ));
                }
                let value = field.bytes().await.map_err(map_multipart_error)?;
                let value = std::str::from_utf8(&value).map_err(|_| {
                    validation_error("chunk_sha256", "Must be lowercase SHA-256 hex.")
                })?;
                validate_lowercase_sha256_hex(value).map_err(|_| {
                    validation_error("chunk_sha256", "Must be lowercase SHA-256 hex.")
                })?;
                chunk_sha256 = Some(value.to_owned());
            }
            "file" => {
                if file.is_some() {
                    return Err(validation_error("file", "Must be supplied exactly once."));
                }
                file = Some(field.bytes().await.map_err(map_multipart_error)?.to_vec());
            }
            _ => {}
        }
    }

    Ok(ParsedChunk {
        chunk_index: chunk_index
            .ok_or_else(|| validation_error("chunk_index", "Must be supplied."))?,
        chunk_sha256: chunk_sha256
            .ok_or_else(|| validation_error("chunk_sha256", "Must be supplied."))?,
        bytes: file.ok_or_else(|| validation_error("file", "Must be supplied."))?,
    })
}

fn map_multipart_error(error: axum::extract::multipart::MultipartError) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::payload_too_large()
    } else {
        ApiError::validation("Malformed multipart body.", None)
    }
}

fn map_multipart_rejection(error: axum::extract::multipart::MultipartRejection) -> ApiError {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::payload_too_large()
    } else {
        ApiError::validation("Malformed multipart body.", None)
    }
}

fn ensure_complete_chunk_set(
    job: &TranscriptionJobRecord,
    chunks: &[ChunkRecord],
) -> Result<(), ApiError> {
    if chunks.len() as i64 != job.chunk_count {
        return Err(ApiError::conflict(
            "Every declared chunk must be accepted before finalize.",
            None,
        ));
    }
    for (expected, chunk) in chunks.iter().enumerate() {
        if chunk.chunk_index != expected as i64 {
            return Err(ApiError::conflict(
                "Every declared chunk must be accepted before finalize.",
                None,
            ));
        }
    }
    Ok(())
}

fn job_resource(record: TranscriptionJobRecord) -> Result<TranscriptionJobResource, ApiError> {
    Ok(TranscriptionJobResource {
        id: record.id,
        status: record.status,
        created_at: timestamp(record.created_at)?,
        updated_at: timestamp(record.updated_at)?,
        chunk_count: record.chunk_count,
        chunks_received: record.chunks_received,
        retry_count: record.retry_count,
        max_retries: record.max_retries,
        next_attempt_at: record.next_attempt_at.map(timestamp).transpose()?,
        failure_code: record.failure_code,
        failure_message: record.failure_message,
        retryable_by_client: record.retryable_by_client,
        voice_note_id: record.voice_note_id,
    })
}
