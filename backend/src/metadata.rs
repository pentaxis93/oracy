use axum::Json;
use axum::extract::{Path, RawQuery, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use ulid::Ulid;

use crate::auth::AuthenticatedKey;
use crate::collections::{
    ensure_singular_query_param, parse_limit, parse_query_params, parse_time_cursor, query_first,
    time_cursor, timestamp, validate_ulid_field,
};
use crate::errors::{ApiError, CollectionEnvelope};
use crate::json::JsonBody;
use crate::state::AppState;
use crate::storage::{
    CreateTagOutcome, NewSession, NewTag, RenameTagOutcome, SessionRecord, TagRecord,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetadataCollection {
    Tags,
    Sessions,
}

#[derive(Debug, Clone)]
struct PageQuery {
    limit: i64,
    cursor: Option<(OffsetDateTime, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagResource {
    id: String,
    name: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionResource {
    id: String,
    name: String,
    created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameRequest {
    name: String,
}

pub async fn list_tags(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<TagResource>>, ApiError> {
    let page = parse_page_query(raw_query.as_deref(), MetadataCollection::Tags)?;
    let rows = state
        .storage
        .list_tags(
            authenticated_key.api_key_id.as_str(),
            page.cursor,
            page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list tags."))?;
    render_tag_page(rows, page.limit, MetadataCollection::Tags)
}

pub async fn create_tag(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    JsonBody(body): JsonBody<NameRequest>,
) -> Result<(StatusCode, Json<TagResource>), ApiError> {
    let outcome = state
        .storage
        .create_tag(NewTag {
            id: Ulid::new().to_string(),
            api_key_id: authenticated_key.api_key_id.to_string(),
            name: body.name,
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to create tag."))?;

    match outcome {
        CreateTagOutcome::Created(tag) => Ok((StatusCode::CREATED, Json(tag_resource(tag)?))),
        CreateTagOutcome::Existing(tag) => Ok((StatusCode::OK, Json(tag_resource(tag)?))),
    }
}

pub async fn get_tag(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(tag_id): Path<String>,
) -> Result<Json<TagResource>, ApiError> {
    validate_ulid_field("tag_id", &tag_id)?;
    let Some(tag) = state
        .storage
        .get_tag(authenticated_key.api_key_id.as_str(), &tag_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load tag."))?
    else {
        return Err(ApiError::not_found("Tag not found."));
    };

    Ok(Json(tag_resource(tag)?))
}

pub async fn patch_tag(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(tag_id): Path<String>,
    JsonBody(body): JsonBody<NameRequest>,
) -> Result<Json<TagResource>, ApiError> {
    validate_ulid_field("tag_id", &tag_id)?;
    match state
        .storage
        .rename_tag(authenticated_key.api_key_id.as_str(), &tag_id, &body.name)
        .await
        .map_err(|_| ApiError::internal("Failed to rename tag."))?
    {
        RenameTagOutcome::Renamed(tag) => Ok(Json(tag_resource(tag)?)),
        RenameTagOutcome::NotFound => Err(ApiError::not_found("Tag not found.")),
        RenameTagOutcome::Conflict => Err(ApiError::conflict("Tag name already exists.", None)),
    }
}

pub async fn delete_tag(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(tag_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate_ulid_field("tag_id", &tag_id)?;
    if !state
        .storage
        .delete_tag(authenticated_key.api_key_id.as_str(), &tag_id)
        .await
        .map_err(|_| ApiError::internal("Failed to delete tag."))?
    {
        return Err(ApiError::not_found("Tag not found."));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_sessions(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> Result<Json<CollectionEnvelope<SessionResource>>, ApiError> {
    let page = parse_page_query(raw_query.as_deref(), MetadataCollection::Sessions)?;
    let rows = state
        .storage
        .list_sessions(
            authenticated_key.api_key_id.as_str(),
            page.cursor,
            page.limit + 1,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to list sessions."))?;
    render_session_page(rows, page.limit, MetadataCollection::Sessions)
}

pub async fn create_session(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    JsonBody(body): JsonBody<NameRequest>,
) -> Result<(StatusCode, Json<SessionResource>), ApiError> {
    let session = state
        .storage
        .create_session(NewSession {
            id: Ulid::new().to_string(),
            api_key_id: authenticated_key.api_key_id.to_string(),
            name: body.name,
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .map_err(|_| ApiError::internal("Failed to create session."))?;

    Ok((StatusCode::CREATED, Json(session_resource(session)?)))
}

pub async fn get_session(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionResource>, ApiError> {
    validate_ulid_field("session_id", &session_id)?;
    let Some(session) = state
        .storage
        .get_session(authenticated_key.api_key_id.as_str(), &session_id)
        .await
        .map_err(|_| ApiError::internal("Failed to load session."))?
    else {
        return Err(ApiError::not_found("Session not found."));
    };

    Ok(Json(session_resource(session)?))
}

pub async fn patch_session(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    JsonBody(body): JsonBody<NameRequest>,
) -> Result<Json<SessionResource>, ApiError> {
    validate_ulid_field("session_id", &session_id)?;
    let Some(session) = state
        .storage
        .rename_session(
            authenticated_key.api_key_id.as_str(),
            &session_id,
            &body.name,
        )
        .await
        .map_err(|_| ApiError::internal("Failed to rename session."))?
    else {
        return Err(ApiError::not_found("Session not found."));
    };

    Ok(Json(session_resource(session)?))
}

pub async fn delete_session(
    authenticated_key: AuthenticatedKey,
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    validate_ulid_field("session_id", &session_id)?;
    if !state
        .storage
        .delete_session(authenticated_key.api_key_id.as_str(), &session_id)
        .await
        .map_err(|_| ApiError::internal("Failed to delete session."))?
    {
        return Err(ApiError::not_found("Session not found."));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn render_tag_page(
    mut rows: Vec<TagRecord>,
    limit: i64,
    collection: MetadataCollection,
) -> Result<Json<CollectionEnvelope<TagResource>>, ApiError> {
    let has_next = rows.len() as i64 > limit;
    if has_next {
        rows.truncate(limit as usize);
    }
    let next_cursor = next_cursor(
        &rows,
        has_next,
        collection,
        |tag| tag.created_at,
        |tag| tag.id.as_str(),
    )?;
    let items = rows
        .into_iter()
        .map(tag_resource)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

fn render_session_page(
    mut rows: Vec<SessionRecord>,
    limit: i64,
    collection: MetadataCollection,
) -> Result<Json<CollectionEnvelope<SessionResource>>, ApiError> {
    let has_next = rows.len() as i64 > limit;
    if has_next {
        rows.truncate(limit as usize);
    }
    let next_cursor = next_cursor(
        &rows,
        has_next,
        collection,
        |session| session.created_at,
        |session| session.id.as_str(),
    )?;
    let items = rows
        .into_iter()
        .map(session_resource)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(CollectionEnvelope { items, next_cursor }))
}

fn next_cursor<T>(
    rows: &[T],
    has_next: bool,
    collection: MetadataCollection,
    created_at: impl Fn(&T) -> OffsetDateTime,
    id: impl Fn(&T) -> &str,
) -> Result<Option<String>, ApiError> {
    if !has_next {
        return Ok(None);
    }
    rows.last()
        .map(|row| time_cursor(collection.as_str(), created_at(row), id(row)))
        .transpose()
}

fn parse_page_query(
    raw_query: Option<&str>,
    collection: MetadataCollection,
) -> Result<PageQuery, ApiError> {
    let params = parse_query_params(raw_query);
    for field in ["cursor", "limit"] {
        ensure_singular_query_param(&params, field)?;
    }
    Ok(PageQuery {
        limit: parse_limit(&params)?,
        cursor: query_first(&params, "cursor")
            .map(|cursor| parse_time_cursor(cursor, collection.as_str()))
            .transpose()?,
    })
}

fn tag_resource(record: TagRecord) -> Result<TagResource, ApiError> {
    Ok(TagResource {
        id: record.id,
        name: record.name,
        created_at: timestamp(record.created_at)?,
    })
}

fn session_resource(record: SessionRecord) -> Result<SessionResource, ApiError> {
    Ok(SessionResource {
        id: record.id,
        name: record.name,
        created_at: timestamp(record.created_at)?,
    })
}

impl MetadataCollection {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tags => "tags",
            Self::Sessions => "sessions",
        }
    }
}
