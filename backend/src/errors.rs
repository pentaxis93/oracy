use axum::Json;
use axum::extract::rejection::JsonRejection;
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

    pub fn repeated_singular_parameter(field: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ErrorResponse {
                error_code: "repeated_singular_parameter".to_owned(),
                message: "A singular query parameter was supplied more than once.".to_owned(),
                details: Some(vec![ErrorDetail {
                    field: field.into(),
                    message: "Must be supplied at most once.".to_owned(),
                }]),
            },
        }
    }

    pub fn from_json_rejection(rejection: JsonRejection) -> Self {
        match rejection {
            JsonRejection::JsonSyntaxError(_) => Self::malformed_json(),
            JsonRejection::MissingJsonContentType(_) => Self::unsupported_content_type(),
            JsonRejection::JsonDataError(_) => Self::invalid_request_shape(),
            JsonRejection::BytesRejection(rejection) => {
                if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    Self::payload_too_large()
                } else {
                    Self::request_body_error(rejection.status())
                }
            }
            _ => Self::request_body_error(rejection.status()),
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

    fn malformed_json() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            body: ErrorResponse {
                error_code: "malformed_json".to_owned(),
                message: "Request body must be valid JSON.".to_owned(),
                details: None,
            },
        }
    }

    fn unsupported_content_type() -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            body: ErrorResponse {
                error_code: "unsupported_content_type".to_owned(),
                message: "Request content type must be application/json.".to_owned(),
                details: None,
            },
        }
    }

    fn invalid_request_shape() -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            body: ErrorResponse {
                error_code: "invalid_request_shape".to_owned(),
                message: "Request JSON shape is invalid.".to_owned(),
                details: None,
            },
        }
    }

    pub fn payload_too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            body: ErrorResponse {
                error_code: "payload_too_large".to_owned(),
                message: "Request body is too large.".to_owned(),
                details: None,
            },
        }
    }

    fn request_body_error(status: StatusCode) -> Self {
        Self {
            status,
            body: ErrorResponse {
                error_code: "request_body_error".to_owned(),
                message: "Failed to read request body.".to_owned(),
                details: None,
            },
        }
    }

    pub fn method_not_allowed() -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            body: ErrorResponse {
                error_code: "method_not_allowed".to_owned(),
                message: "Method not allowed.".to_owned(),
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
