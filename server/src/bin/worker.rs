use vectorize_core::config::Config;
use vectorize_core::init;
use vectorize_worker::executor::poll_job;

#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("starting pg-vectorize remote-worker");

    let cfg = Config::from_env();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");

    init::init_project(&pool, Some(&cfg.database_url))
        .await
        .expect("Failed to initialize project");

    let queue = pgmq::PGMQueueExt::new_with_pool(pool.clone()).await;

    loop {
        match poll_job(&pool, &queue, &cfg).await {
            Ok(Some(_)) => {
                log::info!("processed job!");
                // continue processing
            }
            Ok(None) => {
                // no messages, small wait
                log::debug!(
                    "No messages in queue, waiting for {} seconds",
                    cfg.poll_interval
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(cfg.poll_interval)).await;
            }
            Err(e) => {
                // error, long wait
                log::error!("Error processing job: {e:?}");
                tokio::time::sleep(tokio::time::Duration::from_secs(cfg.poll_interval)).await;
            }
        }
    }
}
