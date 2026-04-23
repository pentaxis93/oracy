use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorResponse {
    pub error_code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<ErrorDetail>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorDetail {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionEnvelope<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: ErrorResponse,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: ErrorResponse {
                error_code: "unauthorized".to_owned(),
                message: "Missing or invalid API key.".to_owned(),
                details: None,
            },
        }
    }

    pub fn validation(message: impl Into<String>, details: Option<Vec<ErrorDetail>>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ErrorResponse {
                error_code: "validation_error".to_owned(),
                message: message.into(),
                details,
            },
        }
    }

    pub fn conflict(message: impl Into<String>, details: Option<Vec<ErrorDetail>>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            body: ErrorResponse {
                error_code: "conflict".to_owned(),
                message: message.into(),
                details,
            },
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            body: ErrorResponse {
                error_code: "not_found".to_owned(),
                message: message.into(),
                details: None,
            },
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: ErrorResponse {
                error_code: "internal_error".to_owned(),
                message: message.into(),
                details: None,
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
