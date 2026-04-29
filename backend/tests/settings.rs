use axum::body::Body;
use axum::http::{Request, StatusCode};
use oracy_backend::bootstrap::load_runtime_from_path;
use oracy_backend::router::build_router;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::util::ServiceExt;

#[tokio::test]
async fn first_read_returns_default_settings_for_authenticated_api_key() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/settings")
                .header("Authorization", "Bearer alpha-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "transcription_model": "gpt-4o-mini-transcribe"
        })
    );
}

#[tokio::test]
async fn patch_updates_transcription_model_to_each_supported_value() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;

    for model in ["gpt-4o-transcribe", "gpt-4o-mini-transcribe"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/v1/settings")
                    .header("Authorization", "Bearer alpha-secret")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        json!({ "transcription_model": model }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            json_body(response).await,
            json!({
                "transcription_model": model
            })
        );
    }
}

#[tokio::test]
async fn empty_patch_preserves_current_settings() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;

    patch_settings(
        &app,
        "alpha-secret",
        json!({ "transcription_model": "gpt-4o-transcribe" }),
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/settings")
                .header("Authorization", "Bearer alpha-secret")
                .header("Content-Type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "transcription_model": "gpt-4o-transcribe"
        })
    );
}

#[tokio::test]
async fn patch_rejects_invalid_setting_values() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;

    for body in [
        json!({ "transcription_model": "not-supported" }),
        json!({ "transcription_model": null }),
        json!({ "unknown": "value" }),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/v1/settings")
                    .header("Authorization", "Bearer alpha-secret")
                    .header("Content-Type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error_code"], "validation_error");
    }
}

#[tokio::test]
async fn settings_routes_require_valid_authentication() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;

    for request in [
        Request::builder()
            .uri("/api/v1/settings")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/api/v1/settings")
            .header("Authorization", "Bearer unknown-secret")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method("PATCH")
            .uri("/api/v1/settings")
            .header("Content-Type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .unwrap(),
    ] {
        let response = app.clone().oneshot(request).await.expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json_body(response).await,
            json!({
                "error_code": "unauthorized",
                "message": "Missing or invalid API key."
            })
        );
    }
}

#[tokio::test]
async fn settings_persist_across_backend_restart() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;
    patch_settings(
        &app,
        "alpha-secret",
        json!({ "transcription_model": "gpt-4o-transcribe" }),
    )
    .await;
    drop(app);

    let restarted = fixture.app().await;
    let response = restarted
        .oneshot(
            Request::builder()
                .uri("/api/v1/settings")
                .header("Authorization", "Bearer alpha-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "transcription_model": "gpt-4o-transcribe"
        })
    );
}

#[tokio::test]
async fn settings_are_isolated_per_api_key() {
    let fixture = SettingsFixture::new().await;
    let app = fixture.app().await;

    patch_settings(
        &app,
        "alpha-secret",
        json!({ "transcription_model": "gpt-4o-transcribe" }),
    )
    .await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/settings")
                .header("Authorization", "Bearer beta-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "transcription_model": "gpt-4o-mini-transcribe"
        })
    );
}

struct SettingsFixture {
    _tempdir: TempDir,
    config_path: std::path::PathBuf,
}

impl SettingsFixture {
    async fn new() -> Self {
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

[[api_keys]]
api_key_id = "beta"
key = "beta-secret"
"#,
                audio_dir.display(),
                tempdir.path().join("oracy.sqlite").display()
            )
            .trim_start(),
        )
        .expect("write config");

        Self {
            _tempdir: tempdir,
            config_path,
        }
    }

    async fn app(&self) -> axum::Router {
        let (_, state) = load_runtime_from_path(&self.config_path)
            .await
            .expect("valid runtime");
        build_router(state)
    }
}

async fn patch_settings(app: &axum::Router, bearer: &str, body: Value) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/settings")
                .header("Authorization", format!("Bearer {bearer}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("valid json")
}
