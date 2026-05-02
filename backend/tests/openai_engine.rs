use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Multipart, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use oracy_backend::transcription_worker::{
    AudioSliceError, AudioSlicer, EngineFailure, OpenAiTranscriptionEngine, TranscriptionEngine,
    TranscriptionInput,
};
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn openai_engine_posts_transcription_requests_and_concatenates_slices() {
    let tempdir = TempDir::new().expect("tempdir");
    let first = tempdir.path().join("first.wav");
    let second = tempdir.path().join("second.wav");
    tokio::fs::write(&first, b"first-audio")
        .await
        .expect("write first slice");
    tokio::fs::write(&second, b"second-audio")
        .await
        .expect("write second slice");
    let observed = Arc::new(Mutex::new(Vec::new()));
    let base_url = spawn_fake_openai(observed.clone()).await;
    let engine = OpenAiTranscriptionEngine::new(
        base_url,
        "test-openai-key".to_owned(),
        FakeSlicer {
            slices: vec![first, second],
        },
    );

    let output = engine
        .transcribe(TranscriptionInput {
            audio_path: tempdir.path().join("accepted.wav"),
            audio_format: "wav".to_owned(),
            language: Some("en".to_owned()),
            model: "gpt-4o-mini-transcribe".to_owned(),
        })
        .await
        .expect("transcribe");

    assert_eq!(output.text, "slice-1\nslice-2");
    let observed = observed.lock().expect("observed requests");
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].model, "gpt-4o-mini-transcribe");
    assert_eq!(observed[0].language.as_deref(), Some("en"));
    assert_eq!(observed[0].file_bytes, b"first-audio");
    assert_eq!(observed[1].file_bytes, b"second-audio");
}

#[tokio::test]
async fn openai_engine_preserves_retry_after_rate_limit_semantics() {
    let tempdir = TempDir::new().expect("tempdir");
    let audio = tempdir.path().join("audio.wav");
    tokio::fs::write(&audio, b"audio")
        .await
        .expect("write audio");
    let app = Router::new().route("/v1/audio/transcriptions", post(rate_limited));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake openai");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve fake openai");
    });
    let engine = OpenAiTranscriptionEngine::new(
        format!("http://{addr}"),
        "test-openai-key".to_owned(),
        FakeSlicer {
            slices: vec![audio],
        },
    );

    let error = engine
        .transcribe(TranscriptionInput {
            audio_path: tempdir.path().join("accepted.wav"),
            audio_format: "wav".to_owned(),
            language: None,
            model: "gpt-4o-mini-transcribe".to_owned(),
        })
        .await
        .expect_err("rate limit should be transient");

    assert_eq!(
        error,
        EngineFailure::Transient {
            failure_code: "engine_rate_limited".to_owned(),
            message: "slow down".to_owned(),
            retry_after_seconds: Some(7),
        }
    );
}

#[tokio::test]
async fn openai_engine_cleans_generated_slices_when_slice_transcription_fails() {
    let tempdir = TempDir::new().expect("tempdir");
    let accepted_audio = tempdir.path().join("accepted.wav");
    tokio::fs::write(&accepted_audio, b"accepted-audio")
        .await
        .expect("write accepted audio");
    let slice_dir = tempdir.path().join(".oracy-slices-test");
    tokio::fs::create_dir(&slice_dir)
        .await
        .expect("create generated slice dir");
    let first = slice_dir.join("slice-0.wav");
    let second = slice_dir.join("slice-1.wav");
    tokio::fs::write(&first, b"first-audio")
        .await
        .expect("write first slice");
    tokio::fs::write(&second, b"second-audio")
        .await
        .expect("write second slice");

    let app = Router::new()
        .route(
            "/v1/audio/transcriptions",
            post(fail_after_first_transcription),
        )
        .with_state(Arc::new(Mutex::new(0_usize)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake openai");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve fake openai");
    });
    let engine = OpenAiTranscriptionEngine::new(
        format!("http://{addr}"),
        "test-openai-key".to_owned(),
        FakeSlicer {
            slices: vec![first.clone(), second.clone()],
        },
    );

    let error = engine
        .transcribe(TranscriptionInput {
            audio_path: accepted_audio,
            audio_format: "wav".to_owned(),
            language: None,
            model: "gpt-4o-mini-transcribe".to_owned(),
        })
        .await
        .expect_err("second slice failure should be returned");

    assert_eq!(
        error,
        EngineFailure::Transient {
            failure_code: "engine_error".to_owned(),
            message: "engine unavailable".to_owned(),
            retry_after_seconds: None,
        }
    );
    assert!(!first.exists());
    assert!(!second.exists());
    assert!(!slice_dir.exists());
}

#[derive(Clone)]
struct ObservedRequest {
    model: String,
    language: Option<String>,
    file_bytes: Vec<u8>,
}

async fn spawn_fake_openai(observed: Arc<Mutex<Vec<ObservedRequest>>>) -> String {
    let app = Router::new()
        .route("/v1/audio/transcriptions", post(fake_transcription))
        .with_state(observed);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake openai");
    let addr: SocketAddr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve fake openai");
    });
    format!("http://{addr}")
}

async fn fake_transcription(
    State(observed): State<Arc<Mutex<Vec<ObservedRequest>>>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Json<serde_json::Value> {
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer test-openai-key")
    );
    let mut model = None;
    let mut language = None;
    let mut file_bytes = None;

    while let Some(field) = multipart.next_field().await.expect("multipart field") {
        let name = field.name().expect("field name").to_owned();
        match name.as_str() {
            "model" => model = Some(field.text().await.expect("model text")),
            "language" => language = Some(field.text().await.expect("language text")),
            "file" => file_bytes = Some(field.bytes().await.expect("file bytes").to_vec()),
            _ => {}
        }
    }

    let mut observed = observed.lock().expect("observed requests");
    observed.push(ObservedRequest {
        model: model.expect("model field"),
        language,
        file_bytes: file_bytes.expect("file field"),
    });
    Json(json!({ "text": format!("slice-{}", observed.len()) }))
}

async fn rate_limited() -> impl IntoResponse {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [("retry-after", HeaderValue::from_static("7"))],
        "slow down",
    )
}

async fn fail_after_first_transcription(
    State(count): State<Arc<Mutex<usize>>>,
) -> impl IntoResponse {
    let mut count = count.lock().expect("transcription count");
    *count += 1;
    if *count == 1 {
        Json(json!({ "text": "first slice" })).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "engine unavailable").into_response()
    }
}

#[derive(Clone)]
struct FakeSlicer {
    slices: Vec<PathBuf>,
}

impl AudioSlicer for FakeSlicer {
    async fn slices(
        &self,
        _input: &Path,
        _audio_format: &str,
    ) -> Result<Vec<PathBuf>, AudioSliceError> {
        Ok(self.slices.clone())
    }
}
