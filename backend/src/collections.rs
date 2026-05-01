use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use ulid::Ulid;

use crate::errors::{ApiError, ErrorDetail};

pub const DEFAULT_LIMIT: i64 = 50;
pub const MAX_LIMIT: i64 = 100;

const CURSOR_VERSION: u8 = 1;

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

pub fn parse_query_params(raw_query: Option<&str>) -> Vec<(String, String)> {
    raw_query
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn ensure_singular_query_param(
    params: &[(String, String)],
    field: &str,
) -> Result<(), ApiError> {
    if query_values(params, field).nth(1).is_some() {
        return Err(ApiError::repeated_singular_parameter(field));
    }

    Ok(())
}

pub fn query_first<'a>(params: &'a [(String, String)], field: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(name, _)| name == field)
        .map(|(_, value)| value.as_str())
}

pub fn query_any(params: &[(String, String)], field: &str) -> bool {
    query_first(params, field).is_some()
}

pub fn query_values<'a>(
    params: &'a [(String, String)],
    field: &'a str,
) -> impl Iterator<Item = &'a str> {
    params
        .iter()
        .filter(move |(name, _)| name == field)
        .map(|(_, value)| value.as_str())
}

pub fn parse_limit(params: &[(String, String)]) -> Result<i64, ApiError> {
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

pub fn parse_time_cursor(
    cursor: &str,
    expected_kind: &str,
) -> Result<(OffsetDateTime, String), ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| malformed_cursor())?;
    let decoded: TimeCursor = serde_json::from_slice(&bytes).map_err(|_| malformed_cursor())?;
    if decoded.v != CURSOR_VERSION || decoded.kind != expected_kind {
        return Err(malformed_cursor());
    }
    let created_at =
        OffsetDateTime::parse(&decoded.created_at, &Rfc3339).map_err(|_| malformed_cursor())?;
    let Ok(parsed_id) = Ulid::from_string(&decoded.id) else {
        return Err(malformed_cursor());
    };
    if parsed_id.to_string() != decoded.id {
        return Err(malformed_cursor());
    }

    Ok((created_at, decoded.id))
}

pub fn parse_position_cursor(cursor: &str, expected_kind: &str) -> Result<i64, ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| malformed_cursor())?;
    let decoded: PositionCursor = serde_json::from_slice(&bytes).map_err(|_| malformed_cursor())?;
    if decoded.v != CURSOR_VERSION || decoded.kind != expected_kind || decoded.position < 0 {
        return Err(malformed_cursor());
    }

    Ok(decoded.position)
}

pub fn time_cursor(kind: &str, created_at: OffsetDateTime, id: &str) -> Result<String, ApiError> {
    let cursor = TimeCursor {
        v: CURSOR_VERSION,
        kind: kind.to_owned(),
        created_at: timestamp(created_at)?,
        id: id.to_owned(),
    };
    encode_cursor(&cursor)
}

pub fn position_cursor(kind: &str, position: i64) -> Result<String, ApiError> {
    let cursor = PositionCursor {
        v: CURSOR_VERSION,
        kind: kind.to_owned(),
        position,
    };
    encode_cursor(&cursor)
}

pub fn timestamp(value: OffsetDateTime) -> Result<String, ApiError> {
    value
        .format(&Rfc3339)
        .map_err(|_| ApiError::internal("Failed to format timestamp."))
}

pub fn validate_ulid_field(field: &str, value: &str) -> Result<(), ApiError> {
    let parsed =
        Ulid::from_string(value).map_err(|_| validation_error(field, "Must be a valid ULID."))?;
    if parsed.to_string() != value {
        return Err(validation_error(field, "Must be a valid ULID."));
    }

    Ok(())
}

pub fn validate_rfc3339_field(field: &str, value: &str) -> Result<(), ApiError> {
    parse_rfc3339_field(field, value).map(|_| ())
}

pub fn parse_rfc3339_field(field: &str, value: &str) -> Result<OffsetDateTime, ApiError> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| validation_error(field, "Must be an RFC 3339 UTC timestamp."))?;
    if parsed.offset() != UtcOffset::UTC {
        return Err(validation_error(
            field,
            "Must be an RFC 3339 UTC timestamp.",
        ));
    }

    Ok(parsed)
}

pub fn validation_error(field: &str, message: &str) -> ApiError {
    ApiError::validation(
        "One or more request fields are invalid.",
        Some(vec![ErrorDetail {
            field: field.to_owned(),
            message: message.to_owned(),
        }]),
    )
}

fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, ApiError> {
    let bytes =
        serde_json::to_vec(cursor).map_err(|_| ApiError::internal("Failed to encode cursor."))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn malformed_cursor() -> ApiError {
    validation_error("cursor", "Malformed cursor.")
}
