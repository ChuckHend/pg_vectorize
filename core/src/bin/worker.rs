use log::{error, info};

use vectorize_core::worker::base::{poll_job, Config};

#[tokio::main]
async fn main() {
    env_logger::init();
    info!("starting pg-vectorize remote-worker");

    let cfg = Config::from_env();

    let conn = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await
        .expect("unable to connect to postgres");

    let queue = pgmq::PGMQueueExt::new_with_pool(conn.clone()).await;

    loop {
        match poll_job(&conn, &queue, &cfg).await {
            Ok(Some(_)) => {
                // continue processing
            }
            Ok(None) => {
                // no messages, small wait
                info!(
                    "No messages in queue, waiting for {} seconds",
                    cfg.poll_interval
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(cfg.poll_interval)).await;
            }
            Err(e) => {
                // error, long wait
                error!("Error processing job: {:?}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(cfg.poll_interval)).await;
            }
        }
    }
}
