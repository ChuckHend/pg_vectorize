use crate::embeddings::EmbedCall;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use vectorize_core::types::VectorizeJob;

#[derive(Debug, thiserror::Error)]
pub enum WireProxyError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Connection timeout")]
    Timeout,
}

#[derive(Debug, Clone)]
pub struct PreparedStatement {
    pub statement_name: String,
    pub sql: String,
    pub embed_calls: Vec<EmbedCall>,
}

#[derive(Clone)]
pub struct ProxyConfig {
    pub postgres_addr: std::net::SocketAddr,
    pub timeout: Duration,
    pub jobmap: Arc<RwLock<HashMap<String, VectorizeJob>>>,
    pub db_pool: sqlx::PgPool,
    pub prepared_statements: Arc<RwLock<HashMap<String, PreparedStatement>>>,
}

pub const QUERY_MESSAGE: u8 = b'Q';
pub const PARSE_MESSAGE: u8 = b'P';
pub const BIND_MESSAGE: u8 = b'B';
pub const EXECUTE_MESSAGE: u8 = b'E';
pub const DESCRIBE_MESSAGE: u8 = b'D';
pub const CLOSE_MESSAGE: u8 = b'C';
pub const SYNC_MESSAGE: u8 = b'S';

pub const PROTOCOL_VERSION_3_0: u32 = 196608;
pub const CANCEL_REQUEST_CODE: u32 = 80877102;
pub const SSL_REQUEST_CODE: u32 = 80877103;
pub const GSSENC_REQUEST_CODE: u32 = 80877104;

pub const BUFFER_SIZE: usize = 8192;
pub const MIN_MESSAGE_HEADER_SIZE: usize = 4;
pub const MIN_REGULAR_MESSAGE_SIZE: usize = 5;
pub const MAX_STARTUP_MESSAGE_SIZE: usize = 10000;
pub const MIN_STARTUP_MESSAGE_SIZE: usize = 8;

pub type MessageResult = Option<((Vec<u8>, Option<ParsedMessage>), usize)>;

#[derive(Debug, Clone)]
pub struct ParsedMessage {
    pub message_type: u8,
    pub sql: Option<String>,
    pub has_embed_calls: bool,
    pub rewritten: bool,
}

pub fn is_known_protocol_version(version: u32) -> bool {
    matches!(
        version,
        PROTOCOL_VERSION_3_0 | CANCEL_REQUEST_CODE | SSL_REQUEST_CODE | GSSENC_REQUEST_CODE
    )
}

pub fn is_startup_message(data: &[u8]) -> Option<usize> {
    if data.len() < MIN_STARTUP_MESSAGE_SIZE {
        return None;
    }

    let potential_length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;

    if (MIN_STARTUP_MESSAGE_SIZE..=MAX_STARTUP_MESSAGE_SIZE).contains(&potential_length)
        && data.len() >= MIN_STARTUP_MESSAGE_SIZE
    {
        let potential_version = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        if is_known_protocol_version(potential_version) {
            return Some(potential_length);
        }
    }

    None
}
