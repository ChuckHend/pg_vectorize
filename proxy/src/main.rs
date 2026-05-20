use clap::Parser;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;
use url::Url;

use vectorize_proxy::cache::{
    load_initial_job_cache, setup_job_change_notifications, start_cache_sync_listener,
};
use vectorize_proxy::protocol::ProxyConfig;
use vectorize_proxy::proxy::run_proxy_loop;

#[derive(Parser)]
#[command(
    name = "vectorize-proxy",
    about = "PostgreSQL wire protocol proxy that intercepts vectorize.search() and vectorize.embed() calls"
)]
struct Args {
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://postgres:postgres@localhost:5432/postgres"
    )]
    database_url: String,

    #[arg(long, env = "VECTORIZE_PROXY_PORT", default_value_t = 5433)]
    proxy_port: u16,

    #[arg(long, env = "VECTORIZE_PROXY_TIMEOUT", default_value_t = 30)]
    timeout_secs: u64,

    #[arg(long, env = "DATABASE_POOL_MAX", default_value_t = 8)]
    db_pool_max: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let args = Args::parse();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(args.db_pool_max)
        .connect(&args.database_url)
        .await?;

    setup_job_change_notifications(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set up job change notifications: {e}"))?;

    let initial_cache = load_initial_job_cache(&pool).await?;
    info!("Loaded {} jobs into proxy cache", initial_cache.len());

    let url = Url::parse(&args.database_url)?;
    let postgres_host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing host in database URL"))?
        .to_string();
    let postgres_port = url.port().unwrap_or(5432);
    let postgres_addr: SocketAddr = format!("{postgres_host}:{postgres_port}")
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("Failed to resolve PostgreSQL host address"))?;

    let config = Arc::new(ProxyConfig {
        postgres_addr,
        timeout: Duration::from_secs(args.timeout_secs),
        jobmap: Arc::new(RwLock::new(initial_cache)),
        db_pool: pool,
        prepared_statements: Arc::new(RwLock::new(HashMap::new())),
    });

    let listen_addr: SocketAddr = format!("0.0.0.0:{}", args.proxy_port).parse()?;

    info!("vectorize-proxy listening on {listen_addr}");
    info!("Forwarding to PostgreSQL at {postgres_addr}");

    // Keep the job cache in sync with database changes via pg_notify.
    let config_for_listener = Arc::clone(&config);
    tokio::spawn(async move {
        if let Err(e) = start_cache_sync_listener(config_for_listener).await {
            tracing::error!("Cache sync listener failed: {e}");
        }
    });

    run_proxy_loop(config, listen_addr)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
}
