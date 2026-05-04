use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::auth::AuthenticatedKey;
use crate::collections::{
    ensure_singular_query_param, parse_limit, parse_position_cursor, parse_query_params,
    parse_rfc3339_field, parse_score_cursor, parse_time_cursor, position_cursor, query_any,
    query_first, query_values, score_cursor, time_cursor, timestamp, validate_rfc3339_field,
    validate_ulid_field, validation_error,
};
use crate::embedding::{EmbeddingEngine, EmbeddingInput, OpenAiEmbeddingEngine};
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
    limit: i64,
    history_cursor: Option<(OffsetDateTime, String)>,
    search_cursor: Option<(f64, OffsetDateTime, String)>,
    filters: VoiceNoteFilters,
    search: Option<SearchQuery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    Keyword,
    Semantic,
    Hybrid,
}

#[derive(Debug, Clone)]
struct SearchQuery {
    raw_query: String,
    fts_query: Option<String>,
    mode: SearchMode,
}

#[derive(Debug, Clone)]
struct RankedVoiceNote {
    record: VoiceNoteRecord,
    score: f64,
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
    if let Some(search) = &query.search {
        let rows = execute_voice_note_search(
            authenticated_key.api_key_id.as_str(),
            &state,
            None,
            &query.filters,
            search,
            query.search_cursor,
            query.limit,
        )
        .await?;

        return render_voice_note_search_page(
            authenticated_key.api_key_id.as_str(),
            &state,
            rows,
            query.limit,
            CursorKind::VoiceNoteHistory,
        )
        .await;
    }

    let rows = state
        .storage
        .list_voice_notes(
            authenticated_key.api_key_id.as_str(),
            &query.filters,
            query.history_cursor,
            query.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list voice notes."))?;

    render_voice_note_page(
        authenticated_key.api_key_id.as_str(),
        &state,
        rows,
        query.limit,
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
    if body.text.trim().is_empty() {
        return Err(validation_error("text", "Must not be blank."));
    }
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

    if let Some(search) = &query.search {
        let rows = execute_voice_note_search(
            authenticated_key.api_key_id.as_str(),
            &state,
            Some(&session_id),
            &query.filters,
            search,
            query.search_cursor,
            query.limit,
        )
        .await?;

        return render_voice_note_search_page(
            authenticated_key.api_key_id.as_str(),
            &state,
            rows,
            query.limit,
            CursorKind::SessionVoiceNoteHistory,
        )
        .await;
    }

    let rows = state
        .storage
        .list_session_voice_notes(
            authenticated_key.api_key_id.as_str(),
            &session_id,
            &query.filters,
            query.history_cursor,
            query.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list session voice notes."))?;

    render_voice_note_page(
        authenticated_key.api_key_id.as_str(),
        &state,
        rows,
        query.limit,
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

async fn execute_voice_note_search(
    api_key_id: &str,
    state: &AppState,
    session_id: Option<&str>,
    filters: &VoiceNoteFilters,
    search: &SearchQuery,
    cursor: Option<(f64, OffsetDateTime, String)>,
    limit: i64,
) -> Result<Vec<RankedVoiceNote>, ApiError> {
    let mut ranks: HashMap<String, (VoiceNoteRecord, Option<usize>, Option<usize>)> =
        HashMap::new();
    if matches!(search.mode, SearchMode::Keyword | SearchMode::Hybrid) {
        if let Some(fts_query) = search.fts_query.as_deref() {
            let rows = match session_id {
                Some(session_id) => {
                    state
                        .storage
                        .search_session_voice_notes_keyword(
                            api_key_id,
                            session_id,
                            filters,
                            fts_query,
                            i64::MAX,
                        )
                        .await
                }
                None => {
                    state
                        .storage
                        .search_voice_notes_keyword(api_key_id, filters, fts_query, i64::MAX)
                        .await
                }
            }
            .map_err(|_| ApiError::internal("Failed to search voice notes."))?;
            for (index, record) in rows.into_iter().enumerate() {
                ranks.insert(record.id.clone(), (record, Some(index + 1), None));
            }
        }
    }

    if matches!(search.mode, SearchMode::Semantic | SearchMode::Hybrid)
        && !search.raw_query.trim().is_empty()
    {
        let semantic_rows =
            semantic_search_rows(api_key_id, state, session_id, filters, &search.raw_query).await?;
        for (index, record) in semantic_rows.into_iter().enumerate() {
            ranks
                .entry(record.id.clone())
                .and_modify(|(_, _, semantic_rank)| *semantic_rank = Some(index + 1))
                .or_insert((record, None, Some(index + 1)));
        }
    }

    let mut rows = ranks
        .into_values()
        .map(|(record, keyword_rank, semantic_rank)| RankedVoiceNote {
            record,
            score: reciprocal_rank_score(keyword_rank, semantic_rank),
        })
        .filter(|row| {
            cursor
                .as_ref()
                .map_or(true, |(cursor_score, cursor_created_at, cursor_id)| {
                    row.score < *cursor_score
                        || (row.score == *cursor_score
                            && (row.record.created_at < *cursor_created_at
                                || (row.record.created_at == *cursor_created_at
                                    && row.record.id.as_str() < cursor_id.as_str())))
                })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| compare_ranked_voice_notes(left, right));
    rows.truncate((limit + 1) as usize);

    Ok(rows)
}

async fn semantic_search_rows(
    api_key_id: &str,
    state: &AppState,
    session_id: Option<&str>,
    filters: &VoiceNoteFilters,
    q: &str,
) -> Result<Vec<VoiceNoteRecord>, ApiError> {
    let engine =
        OpenAiEmbeddingEngine::new(state.openai_base_url.clone(), state.openai_api_key.clone());
    let query_embedding = engine
        .embed(EmbeddingInput { text: q.to_owned() })
        .await
        .map_err(|_| ApiError::internal("Failed to search voice notes."))?;
    let rows = state
        .storage
        .list_voice_notes_for_search(api_key_id, session_id, filters)
        .await
        .map_err(|_| ApiError::internal("Failed to search voice notes."))?;
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let embeddings = state
        .storage
        .get_current_embeddings_for_voice_notes(api_key_id, &ids)
        .await
        .map_err(|_| ApiError::internal("Failed to search voice notes."))?;
    let mut scored = rows
        .into_iter()
        .filter_map(|record| {
            let embedding = embeddings
                .get(&record.id)
                .and_then(|bytes| crate::storage::decode_embedding_vector(bytes))?;
            Some((
                cosine_similarity(&query_embedding.vector, &embedding),
                record,
            ))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .total_cmp(left_score)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| right.id.cmp(&left.id))
    });

    Ok(scored.into_iter().map(|(_, record)| record).collect())
}

fn reciprocal_rank_score(keyword_rank: Option<usize>, semantic_rank: Option<usize>) -> f64 {
    const RRF_K: f64 = 60.0;
    keyword_rank
        .map(|rank| 1.0 / (RRF_K + rank as f64))
        .unwrap_or(0.0)
        + semantic_rank
            .map(|rank| 1.0 / (RRF_K + rank as f64))
            .unwrap_or(0.0)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return f64::NEG_INFINITY;
    }
    let mut dot = 0.0;
    let mut left_magnitude = 0.0;
    let mut right_magnitude = 0.0;
    for (left, right) in left.iter().zip(right.iter()) {
        let left = *left as f64;
        let right = *right as f64;
        dot += left * right;
        left_magnitude += left * left;
        right_magnitude += right * right;
    }
    if left_magnitude == 0.0 || right_magnitude == 0.0 {
        f64::NEG_INFINITY
    } else {
        dot / (left_magnitude.sqrt() * right_magnitude.sqrt())
    }
}

fn compare_ranked_voice_notes(
    left: &RankedVoiceNote,
    right: &RankedVoiceNote,
) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| right.record.created_at.cmp(&left.record.created_at))
        .then_with(|| right.record.id.cmp(&left.record.id))
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

async fn render_voice_note_search_page(
    api_key_id: &str,
    state: &AppState,
    mut rows: Vec<RankedVoiceNote>,
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
                score_cursor(
                    cursor_kind.search_cursor_kind(),
                    voice_note.score,
                    voice_note.record.created_at,
                    &voice_note.record.id,
                )
            })
            .transpose()?
    } else {
        None
    };
    let voice_note_ids = rows
        .iter()
        .map(|row| row.record.id.clone())
        .collect::<Vec<_>>();
    let mut tags_by_voice_note = state
        .storage
        .list_tags_for_voice_notes(api_key_id, &voice_note_ids)
        .await
        .map_err(|_| ApiError::internal("Failed to load voice note tags."))?;
    let items = rows
        .into_iter()
        .map(|row| {
            let tags = tags_by_voice_note
                .remove(&row.record.id)
                .unwrap_or_default();
            voice_note_resource(row.record, tags)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

fn parse_voice_note_collection_query(
    params: &[(String, String)],
    cursor_kind: CursorKind,
) -> Result<VoiceNoteCollectionQuery, ApiError> {
    validate_repeated_singular_query_params(params, cursor_kind)?;
    validate_collection_query_values(params, cursor_kind)?;
    let search = parse_search_query(params)?;
    let cursor = query_first(params, "cursor");
    let history_cursor = if search.is_none() {
        cursor
            .map(|cursor| parse_time_cursor(cursor, cursor_kind.as_str()))
            .transpose()?
    } else {
        None
    };
    let search_cursor = if search.is_some() {
        cursor
            .map(|cursor| parse_score_cursor(cursor, cursor_kind.search_cursor_kind()))
            .transpose()?
    } else {
        None
    };
    Ok(VoiceNoteCollectionQuery {
        limit: parse_limit(params)?,
        history_cursor,
        search_cursor,
        filters: parse_voice_note_filters(params, cursor_kind)?,
        search,
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

fn parse_search_query(params: &[(String, String)]) -> Result<Option<SearchQuery>, ApiError> {
    let Some(q) = query_first(params, "q") else {
        return Ok(None);
    };
    let fts_query = plain_text_fts_query(q);
    let mode = match query_first(params, "search_mode").unwrap_or("hybrid") {
        "keyword" => SearchMode::Keyword,
        "semantic" => SearchMode::Semantic,
        "hybrid" => SearchMode::Hybrid,
        _ => unreachable!("search_mode values are validated before parsing"),
    };

    Ok(Some(SearchQuery {
        raw_query: q.to_owned(),
        fts_query,
        mode,
    }))
}

fn plain_text_fts_query(raw: &str) -> Option<String> {
    let terms = raw
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(fts_quoted_phrase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn fts_quoted_phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
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

    fn search_cursor_kind(self) -> &'static str {
        match self {
            Self::VoiceNoteHistory => "voice_note_search",
            Self::SessionVoiceNoteHistory => "session_voice_note_search",
            _ => self.as_str(),
        }
    }
}
