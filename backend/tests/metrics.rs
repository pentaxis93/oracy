use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use oracy_backend::bootstrap::load_runtime_from_path;
use oracy_backend::metrics::RetentionArtifact;
use oracy_backend::router::{build_operator_router, build_router};
use tempfile::TempDir;
use tower::util::ServiceExt;

#[tokio::test]
async fn operator_metrics_endpoint_exposes_initial_prometheus_series() {
    let fixture = MetricsFixture::new().await;
    tokio::fs::create_dir(fixture.accepted_audio_dir.join("job-a"))
        .await
        .expect("create retained audio dir");
    tokio::fs::write(
        fixture
            .accepted_audio_dir
            .join("job-a")
            .join("accepted.wav"),
        b"audio",
    )
    .await
    .expect("write retained audio");

    let response = build_operator_router(fixture.state.clone())
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content type"),
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let body = body_text(response).await;

    assert!(body.contains("# HELP oracy_transcription_worker_jobs_total"));
    assert!(body.contains("# TYPE oracy_transcription_worker_jobs_total counter"));
    assert!(body.contains("oracy_transcription_worker_jobs_total"));
    assert!(body.contains(r#"outcome="succeeded""#));
    assert!(body.contains(r#"failure_class="none""#));
    assert!(body.contains("oracy_retention_cleanup_artifacts_total"));
    assert!(body.contains(r#"artifact="chunk""#));
    assert!(body.contains("# HELP oracy_transcription_abandonments_total"));
    assert!(body.contains("# TYPE oracy_transcription_abandonments_total counter"));
    assert!(body.contains("oracy_transcription_abandonments_total 0"));
    assert!(body.contains("# HELP oracy_retained_audio_bytes"));
    assert!(body.contains("# TYPE oracy_retained_audio_bytes gauge"));
    assert!(body.contains("oracy_retained_audio_bytes 5"));
}

#[tokio::test]
async fn public_api_does_not_expose_metrics_endpoint() {
    let fixture = MetricsFixture::new().await;

    let response = build_router(fixture.state)
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn retention_cleanup_metrics_increment_with_bounded_artifact_labels() {
    let fixture = MetricsFixture::new().await;
    fixture
        .state
        .metrics
        .record_retention_cleanup_succeeded(RetentionArtifact::Chunk);
    fixture
        .state
        .metrics
        .record_retention_cleanup_failed(RetentionArtifact::ComposedAudio);

    let response = build_operator_router(fixture.state.clone())
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = body_text(response).await;

    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="succeeded""#, r#"artifact="chunk""#],
        "1",
    );
    assert_metric_sample_has_value(
        &body,
        "oracy_retention_cleanup_artifacts_total",
        &[r#"outcome="failed""#, r#"artifact="composed_audio""#],
        "1",
    );
}

struct MetricsFixture {
    _tempdir: TempDir,
    accepted_audio_dir: std::path::PathBuf,
    state: oracy_backend::state::AppState,
}

impl MetricsFixture {
    async fn new() -> Self {
        let tempdir = TempDir::new().expect("tempdir");
        let accepted_audio_dir = tempdir.path().join("accepted-audio");
        tokio::fs::create_dir(&accepted_audio_dir)
            .await
            .expect("create accepted audio dir");
        let config_path = tempdir.path().join("oracy.toml");
        tokio::fs::write(
            &config_path,
            format!(
                r#"
accepted_audio_dir = "{}"
database_path = "{}"

[[api_keys]]
api_key_id = "alpha"
key = "alpha-secret"
"#,
                accepted_audio_dir.display(),
                tempdir.path().join("oracy.sqlite").display()
            )
            .trim_start(),
        )
        .await
        .expect("write config");
        let (_, state) = load_runtime_from_path(&config_path)
            .await
            .expect("valid runtime");

        Self {
            _tempdir: tempdir,
            accepted_audio_dir,
            state,
        }
    }
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8 body")
}

fn assert_metric_sample_has_value(body: &str, name: &str, labels: &[&str], value: &str) {
    assert!(
        body.lines().any(|line| {
            line.starts_with(name)
                && labels.iter().all(|label| line.contains(label))
                && line.ends_with(&format!(" {value}"))
        }),
        "expected {name} with labels {labels:?} and value {value} in:\n{body}"
    );
}
