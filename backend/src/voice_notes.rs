use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::auth::AuthenticatedKey;
use crate::collections::{
    ensure_singular_query_param, parse_limit, parse_position_cursor, parse_query_params,
    parse_rfc3339_field, parse_time_cursor, position_cursor, query_any, query_first, query_values,
    time_cursor, timestamp, validate_rfc3339_field, validate_ulid_field, validation_error,
};
use crate::embedding_regeneration::EmbeddingRegenerationRequest;
use crate::errors::{ApiError, CollectionEnvelope};
use crate::json::JsonBody;
use crate::state::AppState;
use crate::storage::{
    ReplaceVoiceNoteTagsOutcome, SegmentRecord, TagRecord, UpdateVoiceNoteTextOutcome,
    VoiceNoteFilters, VoiceNoteRecord, VoiceNoteVersionRecord,
};

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

#[derive(Debug, Clone)]
struct VoiceNoteCollectionQuery {
    page: PageQuery<(OffsetDateTime, String)>,
    filters: VoiceNoteFilters,
    search_requested: bool,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceNoteTextRequest {
    text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceNoteTagsRequest {
    tag_ids: Vec<String>,
}

pub async fn list_voice_notes(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<VoiceNoteResource>>, ApiError> {
    let params = parse_query_params(raw_query.as_deref());
    let query = parse_voice_note_collection_query(&params, CursorKind::VoiceNoteHistory)?;
    if query.search_requested {
        return Ok(empty_voice_note_collection());
    }

    let rows = state
        .storage
        .list_voice_notes(
            authenticated_key.api_key_id.as_str(),
            &query.filters,
            query.page.cursor,
            query.page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list voice notes."))?;

    render_voice_note_page(
        authenticated_key.api_key_id.as_str(),
        &state,
        rows,
        query.page.limit,
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
        .get_voice_note(authenticated_key.api_key_id.as_str(), &voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note."))?
    else {
        return Err(ApiError::not_found("Voice note not found."));
    };
    let tags = state
        .storage
        .list_voice_note_tags(authenticated_key.api_key_id.as_str(), &voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note tags."))?;

    Ok(Json(voice_note_resource(record, tags)?))
}

pub async fn patch_voice_note(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(voice_note_id): Path<String>,
    JsonBody(body): JsonBody<VoiceNoteTextRequest>,
) -> Result<Json<VoiceNoteResource>, ApiError> {
    validate_ulid_field("voice_note_id", &voice_note_id)?;
    let record = match state
        .storage
        .update_voice_note_text(
            authenticated_key.api_key_id.as_str(),
            &voice_note_id,
            &body.text,
            OffsetDateTime::now_utc(),
        )
        .await
        .map_err(|_| ApiError::internal("Failed to update voice note."))?
    {
        UpdateVoiceNoteTextOutcome::Updated(record) => *record,
        UpdateVoiceNoteTextOutcome::NotFound => {
            return Err(ApiError::not_found("Voice note not found."));
        }
    };

    if let Err(error) =
        state
            .embedding_regeneration_trigger
            .initiate(EmbeddingRegenerationRequest {
                api_key_id: authenticated_key.api_key_id.to_string(),
                voice_note_id: voice_note_id.clone(),
            })
    {
        tracing::error!(
            voice_note_id = %voice_note_id,
            "embedding regeneration trigger failed: {error}"
        );
    }

    render_voice_note_resource(&state, authenticated_key.api_key_id.as_str(), record).await
}

pub async fn delete_voice_note(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(voice_note_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate_ulid_field("voice_note_id", &voice_note_id)?;
    if !state
        .storage
        .delete_voice_note(authenticated_key.api_key_id.as_str(), &voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to delete voice note."))?
    {
        return Err(ApiError::not_found("Voice note not found."));
    }

    Ok(StatusCode::NO_CONTENT)
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
        .list_voice_note_versions(
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
                    CursorKind::VoiceNoteVersions.as_str(),
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
            .map(|segment| {
                position_cursor(CursorKind::VoiceNoteSegments.as_str(), segment.position)
            })
            .transpose()?
    } else {
        None
    };
    let items = rows.into_iter().map(segment_resource).collect::<Vec<_>>();

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

pub async fn put_voice_note_tags(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(voice_note_id): Path<String>,
    JsonBody(body): JsonBody<VoiceNoteTagsRequest>,
) -> Result<Json<VoiceNoteResource>, ApiError> {
    validate_ulid_field("voice_note_id", &voice_note_id)?;
    for tag_id in &body.tag_ids {
        validate_ulid_field("tag_ids", tag_id)?;
    }

    match state
        .storage
        .replace_voice_note_tags(
            authenticated_key.api_key_id.as_str(),
            &voice_note_id,
            &body.tag_ids,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to replace voice note tags."))?
    {
        ReplaceVoiceNoteTagsOutcome::Replaced => {}
        ReplaceVoiceNoteTagsOutcome::NotFound => {
            return Err(ApiError::not_found("Voice note not found."));
        }
        ReplaceVoiceNoteTagsOutcome::DuplicateTagIds => {
            return Err(validation_error(
                "tag_ids",
                "Duplicate tag_ids are invalid.",
            ));
        }
    }

    let Some(record) = state
        .storage
        .get_voice_note(authenticated_key.api_key_id.as_str(), &voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note."))?
    else {
        return Err(ApiError::internal("Updated voice note was not found."));
    };
    render_voice_note_resource(&state, authenticated_key.api_key_id.as_str(), record).await
}

pub async fn list_session_voice_notes(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<VoiceNoteResource>>, ApiError> {
    validate_ulid_field("session_id", &session_id)?;
    let params = parse_query_params(raw_query.as_deref());
    let query = parse_voice_note_collection_query(&params, CursorKind::SessionVoiceNoteHistory)?;
    if !state
        .storage
        .session_exists(authenticated_key.api_key_id.as_str(), &session_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load session."))?
    {
        return Err(ApiError::not_found("Session not found."));
    }

    if query.search_requested {
        return Ok(empty_voice_note_collection());
    }

    let rows = state
        .storage
        .list_session_voice_notes(
            authenticated_key.api_key_id.as_str(),
            &session_id,
            &query.filters,
            query.page.cursor,
            query.page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list session voice notes."))?;

    render_voice_note_page(
        authenticated_key.api_key_id.as_str(),
        &state,
        rows,
        query.page.limit,
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
        .get_voice_note(api_key_id, voice_note_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note."))?
        .is_none()
    {
        return Err(ApiError::not_found("Voice note not found."));
    }

    Ok(())
}

async fn render_voice_note_resource(
    state: &AppState,
    api_key_id: &str,
    record: VoiceNoteRecord,
) -> Result<Json<VoiceNoteResource>, ApiError> {
    let tags = state
        .storage
        .list_voice_note_tags(api_key_id, &record.id)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note tags."))?;

    Ok(Json(voice_note_resource(record, tags)?))
}

async fn render_voice_note_page(
    api_key_id: &str,
    state: &AppState,
    mut rows: Vec<VoiceNoteRecord>,
    limit: i64,
    cursor_kind: CursorKind,
) -> Result<Json<CollectionEnvelope<VoiceNoteResource>>, ApiError> {
    let has_next = rows.len() as i64 > limit;
    if has_next {
        rows.truncate(limit as usize);
    }
    let next_cursor = if has_next {
        rows.last()
            .map(|voice_note| {
                time_cursor(cursor_kind.as_str(), voice_note.created_at, &voice_note.id)
            })
            .transpose()?
    } else {
        None
    };
    let voice_note_ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let mut tags_by_voice_note = state
        .storage
        .list_tags_for_voice_notes(api_key_id, &voice_note_ids)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note tags."))?;
    let items = rows
        .into_iter()
        .map(|row| {
            let tags = tags_by_voice_note.remove(&row.id).unwrap_or_default();
            voice_note_resource(row, tags)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

fn empty_voice_note_collection() -> Json<CollectionEnvelope<VoiceNoteResource>> {
    Json(CollectionEnvelope {
        items: Vec::new(),
        next_cursor: None,
    })
}

fn parse_voice_note_collection_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<VoiceNoteCollectionQuery, ApiError> {
    validate_repeated_singular_query_params(params, cursor_kind)?;
    validate_collection_query_values(params, cursor_kind)?;
    Ok(VoiceNoteCollectionQuery {
        page: PageQuery {
            limit: parse_limit(params)?,
            cursor: query_first(params, "cursor")
                .map(|cursor| parse_time_cursor(cursor, cursor_kind.as_str()))
                .transpose()?,
        },
        filters: parse_voice_note_filters(params, cursor_kind)?,
        search_requested: query_any(params, "q"),
    })
}

fn parse_time_page_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<PageQuery<(OffsetDateTime, String)>, ApiError> {
    validate_repeated_singular_query_params(params, cursor_kind)?;
    validate_collection_query_values(params, cursor_kind)?;
    Ok(PageQuery {
        limit: parse_limit(params)?,
        cursor: query_first(params, "cursor")
            .map(|cursor| parse_time_cursor(cursor, cursor_kind.as_str()))
            .transpose()?,
    })
}

fn parse_position_page_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<PageQuery<i64>, ApiError> {
    validate_repeated_singular_query_params(params, cursor_kind)?;
    validate_collection_query_values(params, cursor_kind)?;
    Ok(PageQuery {
        limit: parse_limit(params)?,
        cursor: query_first(params, "cursor")
            .map(|cursor| parse_position_cursor(cursor, cursor_kind.as_str()))
            .transpose()?,
    })
}

fn validate_repeated_singular_query_params(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<(), ApiError> {
    for field in ["cursor", "limit"] {
        ensure_singular_query_param(params, field)?;
    }
    if cursor_kind.is_voice_note_collection() {
        for field in [
            "q",
            "search_mode",
            "recorded_after",
            "recorded_before",
            "created_after",
            "created_before",
        ] {
            ensure_singular_query_param(params, field)?;
        }
        if cursor_kind == CursorKind::VoiceNoteHistory {
            ensure_singular_query_param(params, "session_id")?;
        }
    }

    Ok(())
}

fn validate_collection_query_values(
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

fn parse_voice_note_filters(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<VoiceNoteFilters, ApiError> {
    let mut filters = VoiceNoteFilters::default();
    for tag_id in query_values(params, "tag_id") {
        let tag_id = tag_id.to_owned();
        if !filters.tag_ids.contains(&tag_id) {
            filters.tag_ids.push(tag_id);
        }
    }
    if cursor_kind == CursorKind::VoiceNoteHistory {
        filters.session_id = query_first(params, "session_id").map(str::to_owned);
    }
    filters.recorded_after = parse_optional_rfc3339_filter(params, "recorded_after")?;
    filters.recorded_before = parse_optional_rfc3339_filter(params, "recorded_before")?;
    filters.created_after = parse_optional_rfc3339_filter(params, "created_after")?;
    filters.created_before = parse_optional_rfc3339_filter(params, "created_before")?;

    Ok(filters)
}

fn parse_optional_rfc3339_filter(
    params: &[(String, String)],
    field: &str,
) -> Result<Option<OffsetDateTime>, ApiError> {
    query_first(params, field)
        .map(|value| parse_rfc3339_field(field, value))
        .transpose()
}

fn voice_note_resource(
    record: VoiceNoteRecord,
    tags: Vec<TagRecord>,
) -> Result<VoiceNoteResource, ApiError> {
    Ok(VoiceNoteResource {
        id: record.id,
        current_version_id: record.current_version_id,
        text: record.text,
        audio_duration_seconds: record.audio_duration_seconds,
        audio_format: record.audio_format,
        audio_size_bytes: record.audio_size_bytes,
        language: record.language,
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
    record: VoiceNoteVersionRecord,
) -> Result<VoiceNoteVersionResource, ApiError> {
    Ok(VoiceNoteVersionResource {
        id: record.id,
        voice_note_id: record.voice_note_id,
        text: record.text,
        created_at: timestamp(record.created_at)?,
    })
}

fn segment_resource(record: SegmentRecord) -> SegmentResource {
    SegmentResource {
        id: record.id,
        voice_note_id: record.voice_note_id,
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
