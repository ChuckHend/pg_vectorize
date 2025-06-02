use clap::Parser;
use log::{debug, error, info, warn};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

#[derive(Parser)]
#[command(name = "postgres-proxy")]
#[command(about = "A TCP proxy for PostgreSQL with connection management")]
struct Args {
    #[arg(short, long, default_value = "5433")]
    listen_port: u16,

    #[arg(short, long, default_value = "127.0.0.1")]
    postgres_host: String,

    #[arg(short = 'P', long, default_value = "5432")]
    postgres_port: u16,

    #[arg(short, long, default_value = "0.0.0.0")]
    bind_address: String,

    /// Connection timeout in seconds
    #[arg(short, long, default_value = "30")]
    timeout: u64,
}

#[derive(Clone)]
struct ProxyConfig {
    postgres_addr: SocketAddr,
    timeout: Duration,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let listen_addr: SocketAddr = format!("{}:{}", args.bind_address, args.listen_port).parse()?;
    let postgres_addr: SocketAddr =
        format!("{}:{}", args.postgres_host, args.postgres_port).parse()?;

    let config = Arc::new(ProxyConfig {
        postgres_addr,
        timeout: Duration::from_secs(args.timeout),
    });

    info!("Starting PostgreSQL proxy");
    info!("Listening on: {}", listen_addr);
    info!("Forwarding to PostgreSQL at: {}", postgres_addr);

    let listener = TcpListener::bind(listen_addr).await?;

    loop {
        match listener.accept().await {
            Ok((client_stream, client_addr)) => {
                info!("New connection from: {}", client_addr);

                let config = Arc::clone(&config);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection_with_timeout(client_stream, config).await {
                        error!("Connection error from {}: {}", client_addr, e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

async fn handle_connection_with_timeout(
    client_stream: TcpStream,
    config: Arc<ProxyConfig>,
) -> Result<(), Box<dyn std::error::Error>> {
    match timeout(
        config.timeout,
        handle_connection(client_stream, config.postgres_addr),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            warn!("Connection timed out");
            Err("Connection timeout".into())
        }
    }
}

async fn handle_connection(
    client_stream: TcpStream,
    postgres_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    // Set TCP_NODELAY for better performance
    client_stream.set_nodelay(true)?;

    let postgres_stream = TcpStream::connect(postgres_addr).await?;
    postgres_stream.set_nodelay(true)?;

    debug!("Connected to PostgreSQL server");

    let (mut client_read, mut client_write) = client_stream.into_split();
    let (mut postgres_read, mut postgres_write) = postgres_stream.into_split();

    let client_to_postgres = tokio::spawn(async move {
        match io::copy(&mut client_read, &mut postgres_write).await {
            Ok(bytes) => debug!("Client to PostgreSQL: {} bytes transferred", bytes),
            Err(e) => debug!("Client to PostgreSQL error: {}", e),
        }
    });

    let postgres_to_client = tokio::spawn(async move {
        match io::copy(&mut postgres_read, &mut client_write).await {
            Ok(bytes) => debug!("PostgreSQL to client: {} bytes transferred", bytes),
            Err(e) => debug!("PostgreSQL to client error: {}", e),
        }
    });

    tokio::select! {
        _ = client_to_postgres => debug!("Client to PostgreSQL stream closed"),
        _ = postgres_to_client => debug!("PostgreSQL to client stream closed"),
    }

    info!("Connection closed");
    Ok(())
}
