use std::env;
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
use tokio::sync::broadcast;
use tracing::error;

#[tokio::main]
async fn main() {
    match startup_command(env::args().skip(1)) {
        StartupCommand::Run => {}
        StartupCommand::Version => {
            println!("oracy-backend {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        StartupCommand::UsageError(message) => {
            eprintln!("{message}");
            eprintln!("usage: oracy-backend [--version]");
            std::process::exit(2);
        }
    }

    init_tracing();

    if let Err(error) = run().await {
        error!("{error}");
        std::process::exit(1);
    }
}

enum StartupCommand {
    Run,
    Version,
    UsageError(String),
}

fn startup_command(mut args: impl Iterator<Item = String>) -> StartupCommand {
    match (args.next(), args.next()) {
        (None, None) => StartupCommand::Run,
        (Some(flag), None) if flag == "--version" => StartupCommand::Version,
        (Some(flag), None) => StartupCommand::UsageError(format!("unknown argument: {flag}")),
        _ => StartupCommand::UsageError("too many arguments".to_owned()),
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
    let ShutdownBroadcast {
        sender,
        public_receiver,
        operator_receiver,
    } = shutdown_broadcast();
    tokio::spawn(wait_for_shutdown_signal(sender));

    tokio::try_join!(
        serve(listen_addr, app, public_receiver),
        serve(operator_listen_addr, operator_app, operator_receiver),
    )?;
    Ok(())
}

struct ShutdownBroadcast {
    sender: broadcast::Sender<()>,
    public_receiver: broadcast::Receiver<()>,
    operator_receiver: broadcast::Receiver<()>,
}

fn shutdown_broadcast() -> ShutdownBroadcast {
    let (sender, public_receiver) = broadcast::channel(1);
    let operator_receiver = sender.subscribe();

    ShutdownBroadcast {
        sender,
        public_receiver,
        operator_receiver,
    }
}

async fn serve(
    listen_addr: SocketAddr,
    app: axum::Router,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.recv().await;
        })
        .await?;
    Ok(())
}

async fn wait_for_shutdown_signal(shutdown: broadcast::Sender<()>) {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }

    let _ = shutdown.send(());
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

#[cfg(test)]
mod tests {
    use super::shutdown_broadcast;

    #[test]
    fn shutdown_broadcast_has_server_receivers_before_sender_can_be_spawned() {
        let shutdown = shutdown_broadcast();

        assert_eq!(shutdown.sender.receiver_count(), 2);
    }
}
