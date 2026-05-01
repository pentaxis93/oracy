use axum::Json;
use axum::extract::{Path, RawQuery, State};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use ulid::Ulid;

use crate::auth::AuthenticatedKey;
use crate::errors::{ApiError, CollectionEnvelope, ErrorDetail};
use crate::state::AppState;
use crate::storage::{SegmentRecord, TagRecord, TranscriptRecord, TranscriptVersionRecord};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 100;
const CURSOR_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorKind {
    VoiceNoteHistory,
    SessionVoiceNoteHistory,
    VoiceNoteVersions,
    VoiceNoteSegments,
}

#[derive(Debug, Clone)]
struct PageQuery<T> {
    limit: i64,
    cursor: Option<T>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoiceNoteResource {
    id: String,
    current_version_id: String,
    text: String,
    audio_duration_seconds: f64,
    audio_format: String,
    audio_size_bytes: i64,
    language: Option<String>,
    model: String,
    processing_time_ms: i64,
    cost_cents: Option<i64>,
    created_at: String,
    recorded_at: String,
    session_id: Option<String>,
    tags: Vec<TagResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoiceNoteVersionResource {
    id: String,
    voice_note_id: String,
    text: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SegmentResource {
    id: String,
    voice_note_id: String,
    position: i64,
    start_ms: i64,
    end_ms: i64,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagResource {
    id: String,
    name: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TimeCursor {
    v: u8,
    kind: String,
    created_at: String,
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PositionCursor {
    v: u8,
    kind: String,
    position: i64,
}

pub async fn list_voice_notes(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<VoiceNoteResource>>, ApiError> {
    let params = parse_query_params(raw_query.as_deref());
    let page = parse_time_page_query(&params, CursorKind::VoiceNoteHistory)?;
    if has_deferred_collection_filter(&params, CursorKind::VoiceNoteHistory) {
        return Ok(empty_collection());
    }

    let rows = state
        .storage
        .list_transcripts(
            authenticated_key.api_key_id.as_str(),
            page.cursor,
            page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list voice notes."))?;

    render_voice_note_page(
        authenticated_key.api_key_id.as_str(),
        &state,
        rows,
        page.limit,
        CursorKind::VoiceNoteHistory,
    )
    .await
}

pub async fn get_voice_note(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(voice_note_id): Path<String>,
) -> Result<Json<VoiceNoteResource>, ApiError> {
    validate_ulid_field("voice_note_id", &voice_note_id)?;
    let Some(record) = state
        .storage
        .get_transcript(authenticated_key.api_key_id.as_str(), &voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note."))?
    else {
        return Err(ApiError::not_found("Voice note not found."));
    };
    let tags = state
        .storage
        .list_transcript_tags(authenticated_key.api_key_id.as_str(), &voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note tags."))?;

    Ok(Json(voice_note_resource(record, tags)?))
}

pub async fn list_voice_note_versions(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(voice_note_id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<VoiceNoteVersionResource>>, ApiError> {
    validate_ulid_field("voice_note_id", &voice_note_id)?;
    let params = parse_query_params(raw_query.as_deref());
    let page = parse_time_page_query(&params, CursorKind::VoiceNoteVersions)?;
    ensure_voice_note_exists(
        &state,
        authenticated_key.api_key_id.as_str(),
        &voice_note_id,
    )
    .await?;
    let mut rows = state
        .storage
        .list_transcript_versions(
            authenticated_key.api_key_id.as_str(),
            &voice_note_id,
            page.cursor,
            page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list voice note versions."))?;
    let has_next = rows.len() as i64 > page.limit;
    if has_next {
        rows.truncate(page.limit as usize);
    }
    let next_cursor = if has_next {
        rows.last()
            .map(|version| {
                time_cursor(
                    CursorKind::VoiceNoteVersions,
                    version.created_at,
                    &version.id,
                )
            })
            .transpose()?
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(voice_note_version_resource)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

pub async fn list_voice_note_segments(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(voice_note_id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<SegmentResource>>, ApiError> {
    validate_ulid_field("voice_note_id", &voice_note_id)?;
    let params = parse_query_params(raw_query.as_deref());
    let page = parse_position_page_query(&params, CursorKind::VoiceNoteSegments)?;
    ensure_voice_note_exists(
        &state,
        authenticated_key.api_key_id.as_str(),
        &voice_note_id,
    )
    .await?;
    let mut rows = state
        .storage
        .list_segments_page(
            authenticated_key.api_key_id.as_str(),
            &voice_note_id,
            page.cursor,
            page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list voice note segments."))?;
    let has_next = rows.len() as i64 > page.limit;
    if has_next {
        rows.truncate(page.limit as usize);
    }
    let next_cursor = if has_next {
        rows.last()
            .map(|segment| position_cursor(CursorKind::VoiceNoteSegments, segment.position))
            .transpose()?
    } else {
        None
    };
    let items = rows.into_iter().map(segment_resource).collect::<Vec<_>>();

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

pub async fn list_session_voice_notes(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<VoiceNoteResource>>, ApiError> {
    validate_ulid_field("session_id", &session_id)?;
    let params = parse_query_params(raw_query.as_deref());
    let page = parse_time_page_query(&params, CursorKind::SessionVoiceNoteHistory)?;
    if !state
        .storage
        .session_exists(authenticated_key.api_key_id.as_str(), &session_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load session."))?
    {
        return Err(ApiError::not_found("Session not found."));
    }

    if has_deferred_collection_filter(&params, CursorKind::SessionVoiceNoteHistory) {
        return Ok(empty_collection());
    }

    let rows = state
        .storage
        .list_session_transcripts(
            authenticated_key.api_key_id.as_str(),
            &session_id,
            page.cursor,
            page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list session voice notes."))?;

    render_voice_note_page(
        authenticated_key.api_key_id.as_str(),
        &state,
        rows,
        page.limit,
        CursorKind::SessionVoiceNoteHistory,
    )
    .await
}

async fn ensure_voice_note_exists(
    state: &AppState,
    api_key_id: &str,
    voice_note_id: &str,
) -> Result<(), ApiError> {
    if state
        .storage
        .get_transcript(api_key_id, voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note."))?
        .is_none()
    {
        return Err(ApiError::not_found("Voice note not found."));
    }

    Ok(())
}

async fn render_voice_note_page(
    api_key_id: &str,
    state: &AppState,
    mut rows: Vec<TranscriptRecord>,
    limit: i64,
    cursor_kind: CursorKind,
) -> Result<Json<CollectionEnvelope<VoiceNoteResource>>, ApiError> {
    let has_next = rows.len() as i64 > limit;
    if has_next {
        rows.truncate(limit as usize);
    }
    let next_cursor = if has_next {
        rows.last()
            .map(|voice_note| time_cursor(cursor_kind, voice_note.created_at, &voice_note.id))
            .transpose()?
    } else {
        None
    };
    let transcript_ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let mut tags_by_transcript = state
        .storage
        .list_tags_for_transcripts(api_key_id, &transcript_ids)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note tags."))?;
    let items = rows
        .into_iter()
        .map(|row| {
            let tags = tags_by_transcript.remove(&row.id).unwrap_or_default();
            voice_note_resource(row, tags)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

fn empty_collection<T>() -> Json<CollectionEnvelope<T>> {
    Json(CollectionEnvelope {
        items: Vec::new(),
        next_cursor: None,
    })
}

fn parse_time_page_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<PageQuery<(OffsetDateTime, String)>, ApiError> {
    validate_transitional_query(params, cursor_kind)?;
    Ok(PageQuery {
        limit: parse_limit(params)?,
        cursor: query_first(params, "cursor")
            .map(|cursor| parse_time_cursor(cursor, cursor_kind))
            .transpose()?,
    })
}

fn parse_position_page_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<PageQuery<i64>, ApiError> {
    validate_transitional_query(params, cursor_kind)?;
    Ok(PageQuery {
        limit: parse_limit(params)?,
        cursor: query_first(params, "cursor")
            .map(|cursor| parse_position_cursor(cursor, cursor_kind))
            .transpose()?,
    })
}

fn validate_transitional_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<(), ApiError> {
    if cursor_kind.is_voice_note_collection() {
        let has_q = query_any(params, "q");
        for search_mode in query_values(params, "search_mode") {
            if !has_q {
                return Err(validation_error(
                    "search_mode",
                    "Must be supplied only when q is present.",
                ));
            }
            if !matches!(search_mode, "keyword" | "semantic" | "hybrid") {
                return Err(validation_error(
                    "search_mode",
                    "Must be one of keyword, semantic, hybrid.",
                ));
            }
        }

        for tag_id in query_values(params, "tag_id") {
            validate_ulid_field("tag_id", tag_id)?;
        }

        if cursor_kind == CursorKind::VoiceNoteHistory {
            for session_id in query_values(params, "session_id") {
                validate_ulid_field("session_id", session_id)?;
            }
        }

        for field in [
            "recorded_after",
            "recorded_before",
            "created_after",
            "created_before",
        ] {
            for value in query_values(params, field) {
                validate_rfc3339_field(field, value)?;
            }
        }
    }

    Ok(())
}

fn has_deferred_collection_filter(params: &[(String, String)], cursor_kind: CursorKind) -> bool {
    if !cursor_kind.is_voice_note_collection() {
        return false;
    }

    query_any(params, "q")
        || query_any(params, "search_mode")
        || query_any(params, "tag_id")
        || (cursor_kind == CursorKind::VoiceNoteHistory && query_any(params, "session_id"))
        || [
            "recorded_after",
            "recorded_before",
            "created_after",
            "created_before",
        ]
        .into_iter()
        .any(|field| query_any(params, field))
}

fn parse_limit(params: &[(String, String)]) -> Result<i64, ApiError> {
    let Some(raw_limit) = query_first(params, "limit") else {
        return Ok(DEFAULT_LIMIT);
    };
    let limit = raw_limit
        .parse::<i64>()
        .map_err(|_| validation_error("limit", "Must be an integer in 1..100."))?;
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(validation_error("limit", "Must be an integer in 1..100."));
    }

    Ok(limit)
}

fn parse_query_params(raw_query: Option<&str>) -> Vec<(String, String)> {
    raw_query
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default()
}

fn query_first<'a>(params: &'a [(String, String)], field: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(name, _)| name == field)
        .map(|(_, value)| value.as_str())
}

fn query_any(params: &[(String, String)], field: &str) -> bool {
    query_first(params, field).is_some()
}

fn query_values<'a>(
    params: &'a [(String, String)],
    field: &'a str,
) -> impl Iterator<Item = &'a str> {
    params
        .iter()
        .filter(move |(name, _)| name == field)
        .map(|(_, value)| value.as_str())
}

fn parse_time_cursor(
    cursor: &str,
    expected_kind: CursorKind,
) -> Result<(OffsetDateTime, String), ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| malformed_cursor())?;
    let decoded: TimeCursor = serde_json::from_slice(&bytes).map_err(|_| malformed_cursor())?;
    if decoded.v != CURSOR_VERSION || decoded.kind != expected_kind.as_str() {
        return Err(malformed_cursor());
    }
    let created_at =
        OffsetDateTime::parse(&decoded.created_at, &Rfc3339).map_err(|_| malformed_cursor())?;
    if decoded.id.is_empty() {
        return Err(malformed_cursor());
    }

    Ok((created_at, decoded.id))
}

fn parse_position_cursor(cursor: &str, expected_kind: CursorKind) -> Result<i64, ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| malformed_cursor())?;
    let decoded: PositionCursor = serde_json::from_slice(&bytes).map_err(|_| malformed_cursor())?;
    if decoded.v != CURSOR_VERSION || decoded.kind != expected_kind.as_str() || decoded.position < 0
    {
        return Err(malformed_cursor());
    }

    Ok(decoded.position)
}

fn time_cursor(kind: CursorKind, created_at: OffsetDateTime, id: &str) -> Result<String, ApiError> {
    let cursor = TimeCursor {
        v: CURSOR_VERSION,
        kind: kind.as_str().to_owned(),
        created_at: timestamp(created_at)?,
        id: id.to_owned(),
    };
    encode_cursor(&cursor)
}

fn position_cursor(kind: CursorKind, position: i64) -> Result<String, ApiError> {
    let cursor = PositionCursor {
        v: CURSOR_VERSION,
        kind: kind.as_str().to_owned(),
        position,
    };
    encode_cursor(&cursor)
}

fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, ApiError> {
    let bytes =
        serde_json::to_vec(cursor).map_err(|_| ApiError::internal("Failed to encode cursor."))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn voice_note_resource(
    record: TranscriptRecord,
    tags: Vec<TagRecord>,
) -> Result<VoiceNoteResource, ApiError> {
    Ok(VoiceNoteResource {
        id: record.id,
        current_version_id: record.current_version_id,
        text: record.transcript,
        audio_duration_seconds: record.audio_duration_seconds,
        audio_format: record.audio_format,
        audio_size_bytes: record.audio_size_bytes,
        language: record.transcript_language,
        model: record.model,
        processing_time_ms: record.processing_time_ms,
        cost_cents: record.cost_cents,
        created_at: timestamp(record.created_at)?,
        recorded_at: timestamp(record.recorded_at)?,
        session_id: record.session_id,
        tags: tags
            .into_iter()
            .map(tag_resource)
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn voice_note_version_resource(
    record: TranscriptVersionRecord,
) -> Result<VoiceNoteVersionResource, ApiError> {
    Ok(VoiceNoteVersionResource {
        id: record.id,
        voice_note_id: record.transcript_id,
        text: record.transcript,
        created_at: timestamp(record.created_at)?,
    })
}

fn segment_resource(record: SegmentRecord) -> SegmentResource {
    SegmentResource {
        id: record.id,
        voice_note_id: record.transcript_id,
        position: record.position,
        start_ms: record.start_ms,
        end_ms: record.end_ms,
        text: record.text,
    }
}

fn tag_resource(record: TagRecord) -> Result<TagResource, ApiError> {
    Ok(TagResource {
        id: record.id,
        name: record.name,
        created_at: timestamp(record.created_at)?,
    })
}

fn timestamp(value: OffsetDateTime) -> Result<String, ApiError> {
    value
        .format(&Rfc3339)
        .map_err(|_| ApiError::internal("Failed to format timestamp."))
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

fn validate_ulid_field(field: &str, value: &str) -> Result<(), ApiError> {
    let parsed =
        Ulid::from_string(value).map_err(|_| validation_error(field, "Must be a valid ULID."))?;
    if parsed.to_string() != value {
        return Err(validation_error(field, "Must be a valid ULID."));
    }

    Ok(())
}

fn validate_rfc3339_field(field: &str, value: &str) -> Result<(), ApiError> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| validation_error(field, "Must be an RFC 3339 UTC timestamp."))?;
    if parsed.offset() != UtcOffset::UTC {
        return Err(validation_error(
            field,
            "Must be an RFC 3339 UTC timestamp.",
        ));
    }

    Ok(())
}

fn malformed_cursor() -> ApiError {
    validation_error("cursor", "Malformed cursor.")
}

impl CursorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::VoiceNoteHistory => "voice_note_history",
            Self::SessionVoiceNoteHistory => "session_voice_note_history",
            Self::VoiceNoteVersions => "voice_note_versions",
            Self::VoiceNoteSegments => "voice_note_segments",
        }
    }

    fn is_voice_note_collection(self) -> bool {
        matches!(self, Self::VoiceNoteHistory | Self::SessionVoiceNoteHistory)
    }
}
