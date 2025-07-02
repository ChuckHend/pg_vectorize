use log::{error, info};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::message_parser::{log_message_processing, try_parse_complete_message};
use super::protocol::{BUFFER_SIZE, ProxyConfig, WireProxyError};

pub async fn handle_connection_with_timeout(
    client_stream: TcpStream,
    config: Arc<ProxyConfig>,
) -> Result<(), WireProxyError> {
    match timeout(config.timeout, handle_connection(client_stream, config)).await {
        Ok(result) => result,
        Err(_) => {
            error!("Connection timed out");
            Err(WireProxyError::Timeout)
        }
    }
}

pub async fn handle_connection(
    client_stream: TcpStream,
    config: Arc<ProxyConfig>,
) -> Result<(), WireProxyError> {
    client_stream.set_nodelay(true)?;

    let postgres_stream = TcpStream::connect(config.postgres_addr).await?;
    postgres_stream.set_nodelay(true)?;

    info!("Connected to PostgreSQL server");

    let (client_read, client_write) = client_stream.into_split();
    let (postgres_read, postgres_write) = postgres_stream.into_split();

    let config_clone = Arc::clone(&config);
    let client_to_postgres = tokio::spawn(async move {
        if let Err(e) =
            enhanced_proxy_with_wire_protocol_support(client_read, postgres_write, config_clone)
                .await
        {
            error!("Client to PostgreSQL error: {e}");
        }
    });

    let postgres_to_client = tokio::spawn(async move {
        if let Err(e) = standard_proxy(postgres_read, client_write).await {
            error!("PostgreSQL to client error: {e}");
        }
    });

    tokio::select! {
        _ = client_to_postgres => info!("Client to PostgreSQL stream closed"),
        _ = postgres_to_client => info!("PostgreSQL to client stream closed"),
    }

    info!("Connection closed");
    Ok(())
}

pub async fn enhanced_proxy_with_wire_protocol_support<R, W>(
    mut reader: R,
    mut writer: W,
    config: Arc<ProxyConfig>,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut message_buffer = Vec::new();
    let mut total_bytes = 0;

    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }

        total_bytes += bytes_read;
        message_buffer.extend_from_slice(&buffer[..bytes_read]);

        while !message_buffer.is_empty() {
            match try_parse_complete_message(&message_buffer, &config).await {
                Some((processed_message, consumed_bytes)) => {
                    if let Some(parsed) = &processed_message.1 {
                        log_message_processing(parsed);
                    }

                    writer.write_all(&processed_message.0).await?;
                    writer.flush().await?;

                    message_buffer.drain(..consumed_bytes);
                }
                None => {
                    break;
                }
            }
        }
    }

    info!("Enhanced proxy stream closed: {total_bytes} bytes transferred");
    Ok(())
}

pub async fn standard_proxy<R, W>(
    mut reader: R,
    mut writer: W,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut total_bytes = 0;

    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }

        total_bytes += bytes_read;
        writer.write_all(&buffer[..bytes_read]).await?;
        writer.flush().await?;
    }

    info!("Standard proxy stream closed: {total_bytes} bytes transferred");
    Ok(())
}
