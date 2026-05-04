use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use oracy_backend::embedding::{
    EmbeddingEngine, EmbeddingFailure, EmbeddingInput, OpenAiEmbeddingEngine,
};
use serde_json::{Value, json};

#[tokio::test]
async fn openai_embedding_engine_posts_embedding_requests_and_pools_long_text_chunks() {
    let captured = Arc::new(Mutex::new(Vec::<Value>::new()));
    let app = Router::new()
        .route("/v1/embeddings", post(fake_embedding))
        .with_state(captured.clone());
    let base_url = spawn(app).await;
    let engine = OpenAiEmbeddingEngine::new(base_url, "test-key".to_owned());

    let output = engine
        .embed(EmbeddingInput {
            text: format!("{}{}", "a".repeat(6_000), "b".repeat(6_000)),
        })
        .await
        .expect("embedding output");

    assert_eq!(output.model, "text-embedding-3-small");
    assert!((output.vector[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
    assert!((output.vector[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
    assert!(output.vector[2..].iter().all(|value| *value == 0.0));

    let requests = captured.lock().expect("captured requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], "text-embedding-3-small");
    assert_eq!(
        requests[0]["input"].as_array().expect("input array").len(),
        2
    );
}

#[tokio::test]
async fn openai_embedding_engine_preserves_retry_after_rate_limit_semantics() {
    async fn rate_limited() -> impl IntoResponse {
        let mut headers = HeaderMap::new();
        headers.insert("Retry-After", "37".parse().expect("retry-after"));
        (
            StatusCode::TOO_MANY_REQUESTS,
            headers,
            Json(json!({"error": {"message": "rate limited"}})),
        )
    }

    let app = Router::new().route("/v1/embeddings", post(rate_limited));
    let base_url = spawn(app).await;
    let engine = OpenAiEmbeddingEngine::new(base_url, "test-key".to_owned());

    let error = engine
        .embed(EmbeddingInput {
            text: "hello".to_owned(),
        })
        .await
        .expect_err("rate limit should fail");

    assert_eq!(
        error,
        EmbeddingFailure::Transient {
            failure_code: "engine_rate_limited".to_owned(),
            message: "{\"error\":{\"message\":\"rate limited\"}}".to_owned(),
            retry_after_seconds: Some(37),
        }
    );
}

async fn fake_embedding(
    State(captured): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    captured.lock().expect("captured").push(body.clone());
    let input = body["input"].as_array().expect("input array");
    let data = input
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut embedding = vec![0.0; 1536];
            embedding[index] = 1.0;
            json!({
                "index": index,
                "embedding": embedding,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "model": "text-embedding-3-small",
        "data": data,
    }))
}

async fn spawn(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake openai");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve fake openai");
    });
    format!("http://{addr}")
}
