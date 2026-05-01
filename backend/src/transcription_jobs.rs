use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use axum::Json;
use axum::extract::{Multipart, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use ulid::Ulid;

use crate::audio_hash::compose_audio_content_hash_hex;
use crate::auth::AuthenticatedKey;
use crate::collections::{parse_rfc3339_field, timestamp, validate_ulid_field};
use crate::errors::{ApiError, ErrorDetail};
use crate::json::JsonBody;
use crate::state::AppState;
use crate::storage::{
    AcceptJobOutcome, AcceptedChunk, FinalizeJobOutcome, FinalizedJob, NewOpenTranscriptionJob,
    PushChunkOutcome, TranscriptionJobRecord,
};

const MAX_CHUNK_BYTES: usize = 26_214_400;
const MAX_RETRIES: i64 = 3;

#[derive(Debug, Clone, Serialize)]
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

pub async fn open_transcription_job(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    headers: HeaderMap,
    JsonBody(body): JsonBody<Value>,
) -> Result<Response, ApiError> {
    let idempotency_key = parse_idempotency_key(&headers)?;
    let request = parse_open_body(body)?;
    let outcome = state
        .storage
        .open_transcription_job(NewOpenTranscriptionJob {
            api_key_id: authenticated_key.api_key_id.as_str().to_owned(),
            idempotency_key,
            recorded_at: request.recorded_at,
            session_id: request.session_id,
            language: request.language,
            audio_format: request.audio_format,
            chunk_count: request.chunk_count,
            max_retries: MAX_RETRIES,
            now: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to open transcription job."))?;

    match outcome {
        AcceptJobOutcome::Created(job) => job_response(StatusCode::CREATED, job),
        AcceptJobOutcome::Replayed(job) => {
            let status = if job.status == "accepting_chunks" {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            job_response(status, job)
        }
        AcceptJobOutcome::Conflict(_) => Err(ApiError::conflict(
            "Idempotency key conflicts with an existing submission attempt.",
            None,
        )),
        AcceptJobOutcome::SessionNotFound => Err(ApiError::not_found("Session not found.")),
    }
}

pub async fn push_chunk(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
    multipart: Multipart,
) -> Result<StatusCode, ApiError> {
    validate_ulid_field("job_id", &job_id)?;
    let chunk = parse_chunk_multipart(multipart).await?;
    if chunk.bytes.len() > MAX_CHUNK_BYTES {
        return Err(ApiError::payload_too_large());
    }
    let actual_hash = sha256_hex(&chunk.bytes);
    if actual_hash != chunk.chunk_sha256 {
        return Err(validation_error(
            "chunk_sha256",
            "Must match the SHA-256 of the uploaded chunk bytes.",
        ));
    }
    let Some(job) = state
        .storage
        .get_job(authenticated_key.api_key_id.as_str(), &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load transcription job."))?
    else {
        return Err(ApiError::not_found("Transcription job not found."));
    };
    if job.status != "accepting_chunks" {
        return Err(ApiError::conflict(
            "Chunk cannot be accepted for this transcription job.",
            None,
        ));
    }
    if chunk.chunk_index < 0 || chunk.chunk_index >= job.chunk_count {
        return Err(validation_error(
            "chunk_index",
            "Must be within the declared chunk range.",
        ));
    }

    let chunk_path = state
        .accepted_audio_dir
        .join(&job_id)
        .join("chunks")
        .join(format!("{:06}.chunk", chunk.chunk_index));
    durable_write(&chunk_path, &chunk.bytes)
        .map_err(|_| ApiError::internal("Failed to persist audio chunk."))?;

    let outcome = state
        .storage
        .accept_chunk(AcceptedChunk {
            api_key_id: authenticated_key.api_key_id.as_str().to_owned(),
            job_id,
            chunk_index: chunk.chunk_index,
            chunk_sha256: chunk.chunk_sha256,
            path: chunk_path,
            size_bytes: chunk.bytes.len() as i64,
            now: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to accept audio chunk."))?;

    match outcome {
        PushChunkOutcome::Accepted | PushChunkOutcome::Replayed => Ok(StatusCode::NO_CONTENT),
        PushChunkOutcome::NotFound => Err(ApiError::not_found("Transcription job not found.")),
        PushChunkOutcome::Conflict => Err(ApiError::conflict(
            "Chunk cannot be accepted for this transcription job.",
            None,
        )),
        PushChunkOutcome::InvalidIndex => Err(validation_error(
            "chunk_index",
            "Must be within the declared chunk range.",
        )),
    }
}

pub async fn finalize_job(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Response, ApiError> {
    validate_ulid_field("job_id", &job_id)?;
    let Some(job) = state
        .storage
        .get_job(authenticated_key.api_key_id.as_str(), &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load transcription job."))?
    else {
        return Err(ApiError::not_found("Transcription job not found."));
    };
    if job.status != "accepting_chunks" {
        if job.accepted_audio_path.as_os_str().is_empty() {
            return Err(ApiError::conflict(
                "Transcription job is not accepting chunks.",
                None,
            ));
        }
        return job_response(StatusCode::OK, job);
    }
    if job.chunks_received != job.chunk_count {
        return Err(ApiError::conflict(
            "Every declared chunk must be accepted before finalize.",
            None,
        ));
    }

    let chunk_hashes = state
        .storage
        .chunk_hashes_in_order(authenticated_key.api_key_id.as_str(), &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load accepted chunks."))?;
    if chunk_hashes.len() != job.chunk_count as usize {
        return Err(ApiError::conflict(
            "Every declared chunk must be accepted before finalize.",
            None,
        ));
    }
    let audio_sha256_hex = compose_audio_content_hash_hex(chunk_hashes)
        .map_err(|_| ApiError::internal("Failed to compose audio content hash."))?;

    let chunk_paths = state
        .storage
        .chunk_paths_in_order(authenticated_key.api_key_id.as_str(), &job_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load accepted chunks."))?;
    let composed_path = state
        .accepted_audio_dir
        .join(&job_id)
        .join(format!("accepted.{}", job.audio_format));
    compose_chunks(&composed_path, &chunk_paths)
        .map_err(|_| ApiError::internal("Failed to persist accepted audio."))?;

    let settings = state
        .storage
        .get_settings(authenticated_key.api_key_id.as_str())
        .await
        .map_err(|_| ApiError::internal("Failed to load settings."))?;

    let outcome = state
        .storage
        .finalize_job(FinalizedJob {
            api_key_id: authenticated_key.api_key_id.as_str().to_owned(),
            job_id,
            audio_sha256_hex,
            accepted_audio_path: composed_path,
            resolved_model: settings.transcription_model,
            now: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to finalize transcription job."))?;

    match outcome {
        FinalizeJobOutcome::Finalized(job) => job_response(StatusCode::ACCEPTED, job),
        FinalizeJobOutcome::Replayed(job) => job_response(StatusCode::OK, job),
        FinalizeJobOutcome::NotFound => Err(ApiError::not_found("Transcription job not found.")),
        FinalizeJobOutcome::Conflict | FinalizeJobOutcome::MissingChunks => Err(
            ApiError::conflict("Transcription job cannot be finalized.", None),
        ),
    }
}

pub async fn get_transcription_job(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
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

    Ok(Json(transcription_job_resource(job)?))
}

fn job_response(status: StatusCode, job: TranscriptionJobRecord) -> Result<Response, ApiError> {
    Ok((status, Json(transcription_job_resource(job)?)).into_response())
}

fn transcription_job_resource(
    job: TranscriptionJobRecord,
) -> Result<TranscriptionJobResource, ApiError> {
    Ok(TranscriptionJobResource {
        id: job.id,
        status: job.status,
        created_at: timestamp(job.created_at)?,
        updated_at: timestamp(job.updated_at)?,
        chunk_count: job.chunk_count,
        chunks_received: job.chunks_received,
        retry_count: job.retry_count,
        max_retries: job.max_retries,
        next_attempt_at: job.next_attempt_at.map(timestamp).transpose()?,
        failure_code: job.failure_code,
        failure_message: job.failure_message,
        retryable_by_client: job.retryable_by_client,
        voice_note_id: job.transcript_id,
    })
}

#[derive(Debug)]
struct OpenJobRequest {
    recorded_at: OffsetDateTime,
    chunk_count: i64,
    audio_format: String,
    session_id: Option<String>,
    language: Option<String>,
}

#[derive(Debug)]
struct ParsedChunk {
    chunk_index: i64,
    chunk_sha256: String,
    bytes: Vec<u8>,
}

fn parse_open_body(body: Value) -> Result<OpenJobRequest, ApiError> {
    let Value::Object(mut fields) = body else {
        return Err(validation_error("", "Request body must be a JSON object."));
    };
    let recorded_at = take_string(&mut fields, "recorded_at")
        .and_then(|value| parse_rfc3339_field("recorded_at", &value))?;
    let chunk_count = fields
        .remove("chunk_count")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| validation_error("chunk_count", "Must be an integer in 1..256."))?;
    if !(1..=256).contains(&chunk_count) {
        return Err(validation_error(
            "chunk_count",
            "Must be an integer in 1..256.",
        ));
    }
    let audio_format = take_string(&mut fields, "audio_format")?;
    if !["m4a", "mp3", "wav", "webm"].contains(&audio_format.as_str()) {
        return Err(validation_error(
            "audio_format",
            "Must be a supported audio format.",
        ));
    }
    let session_id = take_optional_string(&mut fields, "session_id")?;
    if let Some(session_id) = &session_id {
        validate_ulid_field("session_id", session_id)?;
    }
    let language = take_optional_string(&mut fields, "language")?;
    if let Some(language) = &language {
        if language.len() != 2 || !language.bytes().all(|byte| byte.is_ascii_lowercase()) {
            return Err(validation_error(
                "language",
                "Must be a lowercase ISO 639-1 language code.",
            ));
        }
    }

    Ok(OpenJobRequest {
        recorded_at,
        chunk_count,
        audio_format,
        session_id,
        language,
    })
}

async fn parse_chunk_multipart(mut multipart: Multipart) -> Result<ParsedChunk, ApiError> {
    let mut chunk_index = None;
    let mut chunk_sha256 = None;
    let mut bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| validation_error("", "Malformed multipart body."))?
    {
        let Some(name) = field.name().map(str::to_owned) else {
            continue;
        };
        match name.as_str() {
            "chunk_index" => {
                let value = field
                    .text()
                    .await
                    .map_err(|_| validation_error("chunk_index", "Must be an integer."))?;
                let parsed = value
                    .parse::<i64>()
                    .map_err(|_| validation_error("chunk_index", "Must be an integer."))?;
                chunk_index = Some(parsed);
            }
            "chunk_sha256" => {
                let value = field.text().await.map_err(|_| {
                    validation_error("chunk_sha256", "Must be lowercase SHA-256 hex.")
                })?;
                if value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                {
                    return Err(validation_error(
                        "chunk_sha256",
                        "Must be lowercase SHA-256 hex.",
                    ));
                }
                chunk_sha256 = Some(value);
            }
            "file" => {
                let value = field
                    .bytes()
                    .await
                    .map_err(|_| validation_error("file", "Failed to read uploaded chunk."))?;
                bytes = Some(value.to_vec());
            }
            _ => {}
        }
    }

    Ok(ParsedChunk {
        chunk_index: chunk_index
            .ok_or_else(|| validation_error("chunk_index", "Field is required."))?,
        chunk_sha256: chunk_sha256
            .ok_or_else(|| validation_error("chunk_sha256", "Field is required."))?,
        bytes: bytes.ok_or_else(|| validation_error("file", "Field is required."))?,
    })
}

fn parse_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get("Idempotency-Key")
        .ok_or_else(|| validation_error("Idempotency-Key", "Header is required."))?
        .to_str()
        .map_err(|_| validation_error("Idempotency-Key", "Header is malformed."))?;
    if value.is_empty()
        || value.len() > 256
        || !value.bytes().all(|byte| matches!(byte, 0x21..=0x7e))
    {
        return Err(validation_error("Idempotency-Key", "Header is malformed."));
    }

    Ok(value.to_owned())
}

fn take_string(
    fields: &mut serde_json::Map<String, Value>,
    field: &str,
) -> Result<String, ApiError> {
    fields
        .remove(field)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| validation_error(field, "Field is required."))
}

fn take_optional_string(
    fields: &mut serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<String>, ApiError> {
    fields
        .remove(field)
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| validation_error(field, "Must be a string."))
        })
        .transpose()
}

fn validation_error(field: &str, message: &str) -> ApiError {
    ApiError::validation(
        "One or more request fields are invalid.",
        Some(vec![ErrorDetail {
            field: field.to_owned(),
            message: message.to_owned(),
        }]),
    )
}

fn compose_chunks(target: &Path, chunk_paths: &[PathBuf]) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    for chunk_path in chunk_paths {
        bytes.extend_from_slice(&fs::read(chunk_path)?);
    }
    durable_write(target, &bytes)
}

fn durable_write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = target
        .parent()
        .expect("durable write target should have parent");
    fs::create_dir_all(parent)?;
    let temp_path = parent.join(format!(".{}.tmp", Ulid::new()));
    {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp_path, target)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
