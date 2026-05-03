use time::{Duration, OffsetDateTime};

use crate::metrics::Metrics;
use crate::storage::{Storage, StorageError};

const DEFAULT_ABANDONMENT_WINDOW: Duration = Duration::hours(24);
const DEFAULT_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct AbandonmentSweeperConfig {
    pub window: Duration,
    pub interval: std::time::Duration,
}

impl Default for AbandonmentSweeperConfig {
    fn default() -> Self {
        Self {
            window: DEFAULT_ABANDONMENT_WINDOW,
            interval: DEFAULT_SWEEP_INTERVAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbandonmentSweepOutcome {
    pub abandoned_count: usize,
}

pub async fn sweep_abandoned_jobs_once(
    storage: &Storage,
    metrics: &Metrics,
    config: AbandonmentSweeperConfig,
    now: OffsetDateTime,
) -> Result<AbandonmentSweepOutcome, StorageError> {
    let cutoff = now - config.window;
    let candidates = storage
        .list_accepting_chunks_jobs_eligible_for_abandonment(cutoff)
        .await?;
    let mut abandoned_count = 0;

    for candidate in candidates {
        if let Some(abandoned) = storage
            .abandon_accepting_chunks_job(&candidate.api_key_id, &candidate.id, cutoff, now)
            .await?
        {
            abandoned_count += 1;
            metrics.record_transcription_abandoned();
            tracing::info!(
                job_id = %abandoned.id,
                elapsed_seconds = (now - abandoned.created_at).whole_seconds(),
                "transcription job abandoned"
            );
        }
    }

    Ok(AbandonmentSweepOutcome { abandoned_count })
}

pub async fn run_abandonment_sweeper_loop(
    storage: Storage,
    metrics: Metrics,
    config: AbandonmentSweeperConfig,
) {
    tracing::info!(
        abandonment_window_seconds = config.window.whole_seconds(),
        sweep_interval_seconds = config.interval.as_secs(),
        "abandonment sweeper started"
    );
    loop {
        match sweep_abandoned_jobs_once(
            &storage,
            &metrics,
            config.clone(),
            OffsetDateTime::now_utc(),
        )
        .await
        {
            Ok(outcome) => {
                tracing::info!(
                    abandoned_count = outcome.abandoned_count,
                    "abandonment sweeper iteration completed"
                );
            }
            Err(error) => {
                tracing::error!("abandonment sweeper iteration failed: {error}");
            }
        }
        tokio::time::sleep(config.interval).await;
    }
}
