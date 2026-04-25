use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use oracy_backend::auth::AuthenticatedKey;
use oracy_backend::bootstrap::load_runtime_from_path;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::util::ServiceExt;

#[tokio::test]
async fn missing_authorization_header_returns_shared_401() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await,
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[tokio::test]
async fn non_bearer_authorization_header_returns_shared_401() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Basic abc123")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await,
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[tokio::test]
async fn blank_bearer_token_returns_shared_401() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer   ")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await,
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[tokio::test]
async fn bearer_token_with_trailing_whitespace_returns_shared_401() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer alpha-secret ")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await,
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[tokio::test]
async fn bearer_token_with_extra_space_before_key_returns_shared_401() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer  alpha-secret")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await,
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[tokio::test]
async fn unknown_bearer_key_returns_shared_401() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer not-configured")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(response).await,
        json!({
            "error_code": "unauthorized",
            "message": "Missing or invalid API key."
        })
    );
}

#[tokio::test]
async fn valid_bearer_key_exposes_api_key_id_to_handlers() {
    let router = protected_router().await;

    let response = router
        .oneshot(
            Request::builder()
                .uri("/protected")
                .header("Authorization", "Bearer alpha-secret")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, json!({ "api_key_id": "alpha" }));
}

#[tokio::test]
async fn bearer_scheme_is_case_insensitive() {
    let router = protected_router().await;

    for authorization in ["bearer alpha-secret", "BEARER alpha-secret"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/protected")
                    .header("Authorization", authorization)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await, json!({ "api_key_id": "alpha" }));
    }
}

async fn protected_router() -> Router {
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

    Router::new()
        .route("/protected", get(protected_handler))
        .with_state(state)
}

async fn protected_handler(
    authenticated_key: AuthenticatedKey,
    State(_): State<oracy_backend::state::AppState>,
) -> Json<Value> {
    Json(json!({ "api_key_id": authenticated_key.api_key_id.as_str() }))
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("valid json")
}
