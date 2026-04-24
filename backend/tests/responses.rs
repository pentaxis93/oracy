use axum::http::{Request, StatusCode};
use oracy_backend::bootstrap::load_runtime_from_path;
use oracy_backend::errors::{CollectionEnvelope, ErrorDetail, ErrorResponse};
use oracy_backend::router::build_router;
use serde_json::json;
use tempfile::TempDir;
use tower::util::ServiceExt;

#[test]
fn error_response_serializes_without_details_when_none() {
    let response = ErrorResponse {
        error_code: "unauthorized".to_owned(),
        message: "Missing or invalid API key.".to_owned(),
        details: None,
    };

    assert_eq!(
        serde_json::to_value(response).expect("serialize"),
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[test]
fn error_response_serializes_with_details_when_present() {
    let response = ErrorResponse {
        error_code: "validation_error".to_owned(),
        message: "One or more request fields are invalid.".to_owned(),
        details: Some(vec![ErrorDetail {
            field: "language".to_owned(),
            message: "Must be a valid ISO 639-1 code.".to_owned(),
        }]),
    };

    assert_eq!(
        serde_json::to_value(response).expect("serialize"),
        json!({
            "error_code": "validation_error",
            "message": "One or more request fields are invalid.",
            "details": [
                {
                    "field": "language",
                    "message": "Must be a valid ISO 639-1 code."
                }
            ]
        })
    );
}

#[test]
fn collection_envelope_serializes_null_cursor_when_absent() {
    let envelope = CollectionEnvelope {
        items: vec![json!({"id": "one"})],
        next_cursor: None,
    };

    assert_eq!(
        serde_json::to_value(envelope).expect("serialize"),
        json!({
            "items": [{"id": "one"}],
            "next_cursor": null
        })
    );
}

#[tokio::test]
async fn unmatched_route_returns_shared_json_404() {
    let tempdir = TempDir::new().expect("tempdir");
    let audio_dir = tempdir.path().join("accepted-audio");
    std::fs::create_dir(&audio_dir).expect("create dir");
    let config_path = tempdir.path().join("oracy.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"
accepted_audio_dir = "{}"
database_path = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "alpha-secret"
"#,
            audio_dir.display(),
            tempdir.path().join("oracy.sqlite").display()
        )
        .trim_start(),
    )
    .expect("write config");

    let (_, state) = load_runtime_from_path(&config_path)
        .await
        .expect("valid runtime");
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("json"),
        json!({
            "error_code": "not_found",
            "message": "Resource not found."
        })
    );
}
