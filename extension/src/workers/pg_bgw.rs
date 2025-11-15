use crate::guc::{init_guc, NUM_BGW_PROC};
use crate::init::VECTORIZE_QUEUE;
use crate::util::{get_pg_conn, ready};
use pgrx::bgworkers::*;
use pgrx::*;
use std::time::Duration;

use crate::workers::run_worker;

#[pg_guard]
pub extern "C-unwind" fn _PG_init() {
    init_guc();

    let num_bgw = NUM_BGW_PROC.get();
    for i in 0..num_bgw {
        log!("pg-vectorize: starting background worker {}", i);
        let bginst = format!("pg-vectorize-bgw-{}", i);
        BackgroundWorkerBuilder::new(&bginst)
            .set_function("background_worker_main")
            .set_library("vectorize")
            .enable_spi_access()
            .load();
    }
}

#[pg_guard]
#[no_mangle]
pub extern "C-unwind" fn background_worker_main(_arg: pg_sys::Datum) {
    BackgroundWorker::attach_signal_handlers(SignalWakeFlags::SIGHUP | SignalWakeFlags::SIGTERM);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();

    // specify database
    let (conn, queue) = runtime.block_on(async {
        let con = get_pg_conn().await.expect("failed to connect to database");
        let queue = pgmq::PGMQueueExt::new_with_pool(con.clone()).await;
        (con, queue)
    });

    log!("Starting BG Workers {}", BackgroundWorker::get_name(),);

    let mut ext_ready: bool = false;
    let mut wait_duration: Duration = Duration::from_secs(6);
    while BackgroundWorker::wait_latch(Some(wait_duration)) {
        if !ext_ready {
            debug5!("pg-vectorize-bgw: waiting for first pg-vectorize job to be created");
            runtime.block_on(async {
                ext_ready = ready(&conn).await;
            });
            // return to wait_latch if extension is not ready
            continue;
        }

        wait_duration = runtime.block_on(async {
            let wait_dur = match run_worker(queue.clone(), &conn, VECTORIZE_QUEUE).await {
                // no messages in queue, so wait 2 seconds
                Ok(None) => 2000,
                // wait 10 seconds between polls when there is a failure
                Err(_) => 10000,
                // when there was a successfully processed message from queue,
                // only wait 10ms before checking for more messages
                // this allows postgres to kill or restart the bgw in between messages
                Ok(Some(_)) => 10,
            };
            Duration::from_millis(wait_dur)
        });
    }
    log!("pg-vectorize: shutting down");
}
