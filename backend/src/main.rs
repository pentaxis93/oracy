use std::net::SocketAddr;

use oracy_backend::bootstrap::{OPENAI_API_KEY_ENV_VAR, load_runtime_from_env};
use oracy_backend::router::build_router;
use oracy_backend::transcription_worker::{OpenAiTranscriptionEngine, run_transcription_worker};
use tracing::error;

#[tokio::main]
async fn main() {
    init_tracing();

    if let Err(error) = run().await {
        error!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (listen_addr, state) = load_runtime_from_env().await?;
    let openai_api_key = std::env::var(OPENAI_API_KEY_ENV_VAR)?;
    let worker_storage = state.storage.clone();
    tokio::spawn(run_transcription_worker(
        worker_storage,
        OpenAiTranscriptionEngine::new(openai_api_key),
    ));
    serve(listen_addr, build_router(state)).await
}

async fn serve(
    listen_addr: SocketAddr,
    app: axum::Router,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .compact()
        .init();
}
