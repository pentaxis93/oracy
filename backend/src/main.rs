use std::net::SocketAddr;

use oracy_backend::bootstrap::load_runtime_from_env;
use oracy_backend::router::build_router;
use oracy_backend::transcription_worker::{
    FfmpegAudioSlicer, FfprobeDurationProbe, OpenAiTranscriptionEngine, WorkerConfig,
    run_worker_loop,
};
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
    let worker_storage = state.storage.clone();
    let worker_engine = OpenAiTranscriptionEngine::new(
        "https://api.openai.com".to_owned(),
        state.openai_api_key.clone(),
        FfmpegAudioSlicer::openai_limit(),
    );
    tokio::spawn(run_worker_loop(
        worker_storage,
        worker_engine,
        FfprobeDurationProbe,
        WorkerConfig::default(),
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
