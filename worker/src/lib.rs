pub mod executor;
pub mod health;
pub mod ops;

pub use health::*;

use crate::executor::poll_job;
use log::{error, info, warn};
use sqlx::PgPool;
use std::time::Duration;
use vectorize_core::config::Config;

pub async fn start_vectorize_worker_with_monitoring(
    cfg: Config,
    pool: PgPool,
    health_monitor: WorkerHealthMonitor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut restart_count = 0;
    let max_restarts = 5;
    let mut restart_delay = Duration::from_secs(1);
    let max_restart_delay = Duration::from_secs(30);

    loop {
        health_monitor.set_status(WorkerStatus::Starting).await;

        match start_vectorize_worker_inner(cfg.clone(), pool.clone(), health_monitor.clone()).await
        {
            Ok(_) => {
                info!("Vectorize worker completed normally");
                break;
            }
            Err(e) => {
                restart_count += 1;
                let error_msg = format!("Worker failed: {}", e);
                error!("{}", error_msg);
                health_monitor.set_error(error_msg).await;
                health_monitor.increment_restart().await;

                if restart_count >= max_restarts {
                    error!("Max restart attempts ({}) reached, giving up", max_restarts);
                    health_monitor.set_status(WorkerStatus::Dead).await;
                    return Err(format!(
                        "Worker failed permanently after {} restarts",
                        max_restarts
                    )
                    .into());
                }

                warn!(
                    "Restarting worker in {:?} (attempt {}/{})",
                    restart_delay, restart_count, max_restarts
                );
                tokio::time::sleep(restart_delay).await;
                restart_delay = std::cmp::min(restart_delay * 2, max_restart_delay);
            }
        }
    }

    Ok(())
}

async fn start_vectorize_worker_inner(
    cfg: Config,
    pool: PgPool,
    health_monitor: WorkerHealthMonitor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Starting vectorize worker");
    health_monitor.set_status(WorkerStatus::Healthy).await;

    let queue = pgmq::PGMQueueExt::new_with_pool(pool.clone()).await;

    loop {
        health_monitor.heartbeat().await;

        match poll_job(&pool, &queue, &cfg).await {
            Ok(Some(_)) => {
                info!("processed job!");
                health_monitor.job_processed().await;
            }
            Ok(None) => {
                info!(
                    "No messages in queue, waiting for {} seconds",
                    cfg.poll_interval
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(cfg.poll_interval)).await;
            }
            Err(e) => {
                let error_msg = format!("Error processing job: {:?}", e);
                error!("{}", error_msg);
                health_monitor.set_error(error_msg).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(cfg.poll_interval)).await;
            }
        }
    }
}

// Legacy function for backward compatibility
pub async fn start_vectorize_worker(
    cfg: Config,
    pool: PgPool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let health_monitor = WorkerHealthMonitor::new();
    start_vectorize_worker_with_monitoring(cfg, pool, health_monitor).await
}
