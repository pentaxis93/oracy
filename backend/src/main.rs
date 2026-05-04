use std::net::SocketAddr;

use oracy_backend::abandonment_sweeper::{AbandonmentSweeperConfig, run_abandonment_sweeper_loop};
use oracy_backend::bootstrap::load_runtime_from_env;
use oracy_backend::embedding::OpenAiEmbeddingEngine;
use oracy_backend::embedding_regeneration::{
    EmbeddingRegenerationConfig, run_embedding_regeneration_loop,
};
use oracy_backend::retention_cleanup::{RetentionCleanupConfig, run_retention_cleanup_loop};
use oracy_backend::router::{build_operator_router, build_router};
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
    let operator_listen_addr = state.operator_listen_addr;
    let worker_storage = state.storage.clone();
    let worker_engine = OpenAiTranscriptionEngine::new(
        "https://api.openai.com".to_owned(),
        state.openai_api_key.clone(),
        FfmpegAudioSlicer::openai_limit(),
    );
    let embedding_engine = OpenAiEmbeddingEngine::new(
        "https://api.openai.com".to_owned(),
        state.openai_api_key.clone(),
    );
    let regeneration_embedding_engine = OpenAiEmbeddingEngine::new(
        "https://api.openai.com".to_owned(),
        state.openai_api_key.clone(),
    );
    tokio::spawn(run_retention_cleanup_loop(
        state.storage.clone(),
        state.accepted_audio_dir.clone(),
        state.metrics.clone(),
        RetentionCleanupConfig::default(),
    ));
    tokio::spawn(run_abandonment_sweeper_loop(
        state.storage.clone(),
        state.metrics.clone(),
        AbandonmentSweeperConfig::default(),
    ));
    tokio::spawn(run_embedding_regeneration_loop(
        state.storage.clone(),
        regeneration_embedding_engine,
        EmbeddingRegenerationConfig::default(),
    ));
    tokio::spawn(run_worker_loop(
        worker_storage,
        state.accepted_audio_dir.clone(),
        worker_engine,
        embedding_engine,
        FfprobeDurationProbe,
        WorkerConfig::default(),
        state.metrics.clone(),
    ));
    serve_public_and_operator(
        listen_addr,
        build_router(state.clone()),
        operator_listen_addr,
        build_operator_router(state),
    )
    .await
}

async fn serve_public_and_operator(
    listen_addr: SocketAddr,
    app: axum::Router,
    operator_listen_addr: SocketAddr,
    operator_app: axum::Router,
) -> Result<(), Box<dyn std::error::Error>> {
    tokio::try_join!(
        serve(listen_addr, app),
        serve(operator_listen_addr, operator_app),
    )?;
    Ok(())
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
