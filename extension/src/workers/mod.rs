pub mod pg_bgw;

use crate::core::types;
use crate::core::worker::base::execute_job;
use anyhow::Result;
use pgmq::{Message, PGMQueueExt};
use pgrx::*;
use sqlx::{Pool, Postgres};

pub async fn run_worker(
    queue: PGMQueueExt,
    conn: &Pool<Postgres>,
    queue_name: &str,
) -> Result<Option<()>> {
    let msg: Message<types::JobMessage> =
        match queue.read::<types::JobMessage>(queue_name, 180_i32).await {
            Ok(Some(msg)) => msg,
            Ok(None) => {
                info!("pg-vectorize: No messages in queue");
                return Ok(None);
            }
            Err(e) => {
                warning!("pg-vectorize: Error reading message: {e}");
                return Err(anyhow::anyhow!("failed to read message"));
            }
        };

    let msg_id: i64 = msg.msg_id;
    let read_ct: i32 = msg.read_ct;
    info!(
        "pg-vectorize: received message for job: {:?}",
        msg.message.job_name
    );
    let job_success = execute_job(&conn.clone(), msg).await;
    let delete_it = match job_success {
        Ok(_) => {
            info!("pg-vectorize: job success");
            true
        }
        Err(e) => {
            warning!("pg-vectorize: job failed: {:?}", e);
            read_ct > 2
        }
    };

    // delete message from queue
    if delete_it {
        match queue.delete(queue_name, msg_id).await {
            Ok(_) => {
                info!("pg-vectorize: deleted message: {}", msg_id);
            }
            Err(e) => {
                warning!("pg-vectorize: Error deleting message: {}", e);
            }
        }
    }
    // return Some(), indicating that worker consumed some message
    // any possibly more messages on queue
    Ok(Some(()))
}
