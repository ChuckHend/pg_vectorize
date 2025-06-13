use crate::embeddings::{
    JobMapEmbeddingProvider, parse_embed_calls, rewrite_query_with_embeddings,
};
use log::info;
use std::sync::Arc;

use super::protocol::{
    BIND_MESSAGE, CLOSE_MESSAGE, DESCRIBE_MESSAGE, EXECUTE_MESSAGE, MIN_MESSAGE_HEADER_SIZE,
    MIN_REGULAR_MESSAGE_SIZE, MessageResult, PARSE_MESSAGE, ParsedMessage, ProxyConfig,
    QUERY_MESSAGE, SYNC_MESSAGE, is_startup_message,
};

pub async fn process_message_by_type(
    message_type: u8,
    message_data: &[u8],
    config: &ProxyConfig,
    total_message_size: usize,
) -> MessageResult {
    match message_type {
        QUERY_MESSAGE => {
            if let Some((rewritten_message, parsed)) =
                process_simple_query_message(message_data, config).await
            {
                Some(((rewritten_message, Some(parsed)), total_message_size))
            } else {
                Some(((message_data.to_vec(), None), total_message_size))
            }
        }
        PARSE_MESSAGE => {
            if let Some((rewritten_message, parsed)) =
                process_parse_message(message_data, config).await
            {
                Some(((rewritten_message, Some(parsed)), total_message_size))
            } else {
                Some(((message_data.to_vec(), None), total_message_size))
            }
        }
        BIND_MESSAGE | EXECUTE_MESSAGE | DESCRIBE_MESSAGE | CLOSE_MESSAGE | SYNC_MESSAGE => {
            let parsed = create_passthrough_message(message_type);
            Some(((message_data.to_vec(), Some(parsed)), total_message_size))
        }
        _ => {
            info!(
                "Processing message type: {} ({})",
                message_type as char, message_type
            );
            Some(((message_data.to_vec(), None), total_message_size))
        }
    }
}

pub fn create_passthrough_message(message_type: u8) -> ParsedMessage {
    ParsedMessage {
        message_type,
        sql: None,
        has_embed_calls: false,
        rewritten: false,
    }
}

pub fn log_message_processing(parsed: &ParsedMessage) {
    let message_name = match parsed.message_type {
        QUERY_MESSAGE => "Simple Query",
        PARSE_MESSAGE => "Parse (Extended Query)",
        BIND_MESSAGE => "Bind (Extended Query)",
        EXECUTE_MESSAGE => "Execute (Extended Query)",
        DESCRIBE_MESSAGE => "Describe (Extended Query)",
        SYNC_MESSAGE => "Sync (Extended Query)",
        _ => "Unknown message type",
    };
    log::debug!(
        "Processing message type: {} ({})",
        message_name,
        parsed.message_type
    );

    if parsed.has_embed_calls && parsed.rewritten {
        info!("Rewrote query with embeddings!");
    }
}

pub async fn try_parse_complete_message(data: &[u8], config: &ProxyConfig) -> MessageResult {
    if data.len() < MIN_MESSAGE_HEADER_SIZE {
        return None;
    }

    if let Some(startup_length) = is_startup_message(data) {
        if data.len() < startup_length {
            return None;
        }

        info!(
            "Processing startup/special message (length: {})",
            startup_length
        );
        return Some(((data[..startup_length].to_vec(), None), startup_length));
    }

    if data.len() < MIN_REGULAR_MESSAGE_SIZE {
        return None;
    }

    let message_type = data[0];
    let message_length = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
    let total_message_size = 1 + message_length;

    if data.len() < total_message_size {
        return None;
    }

    let message_data = &data[..total_message_size];

    process_message_by_type(message_type, message_data, config, total_message_size).await
}

pub async fn process_simple_query_message(
    data: &[u8],
    config: &ProxyConfig,
) -> Option<(Vec<u8>, ParsedMessage)> {
    if data.len() < 6 {
        return None;
    }

    let query_bytes = &data[5..];
    if let Some(null_pos) = query_bytes.iter().position(|&b| b == 0) {
        let sql = String::from_utf8_lossy(&query_bytes[..null_pos]).to_string();

        if let Ok(embed_calls) = parse_embed_calls(&sql) {
            if !embed_calls.is_empty() {
                let jobmap_read = config.jobmap.read().await;
                let embedding_provider =
                    JobMapEmbeddingProvider::new(Arc::new(jobmap_read.clone()));
                drop(jobmap_read);

                if let Ok(rewritten_sql_string) =
                    rewrite_query_with_embeddings(&sql, &embedding_provider).await
                {
                    let rewritten_message = create_query_message(&rewritten_sql_string);
                    let parsed = ParsedMessage {
                        message_type: QUERY_MESSAGE,
                        sql: Some(rewritten_sql_string),
                        has_embed_calls: true,
                        rewritten: true,
                    };
                    return Some((rewritten_message, parsed));
                }
            }
        }

        let parsed = ParsedMessage {
            message_type: QUERY_MESSAGE,
            sql: Some(sql),
            has_embed_calls: false,
            rewritten: false,
        };
        Some((data.to_vec(), parsed))
    } else {
        None
    }
}

pub async fn process_parse_message(
    data: &[u8],
    config: &ProxyConfig,
) -> Option<(Vec<u8>, ParsedMessage)> {
    if data.len() < 6 {
        return None;
    }

    let mut offset = 5;

    while offset < data.len() && data[offset] != 0 {
        offset += 1;
    }
    offset += 1;

    if offset < data.len() {
        let query_start = offset;
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }

        if offset > query_start {
            let sql = String::from_utf8_lossy(&data[query_start..offset]).to_string();

            if let Ok(embed_calls) = parse_embed_calls(&sql) {
                if !embed_calls.is_empty() {
                    let jobmap_read = config.jobmap.read().await;
                    let embedding_provider =
                        JobMapEmbeddingProvider::new(Arc::new(jobmap_read.clone()));
                    drop(jobmap_read);

                    if let Ok(rewritten_sql_string) =
                        rewrite_query_with_embeddings(&sql, &embedding_provider).await
                    {
                        let rewritten_message = create_parse_message_with_rewritten_query(
                            data,
                            query_start,
                            offset,
                            &rewritten_sql_string,
                        );
                        let parsed = ParsedMessage {
                            message_type: PARSE_MESSAGE,
                            sql: Some(rewritten_sql_string),
                            has_embed_calls: true,
                            rewritten: true,
                        };
                        return Some((rewritten_message, parsed));
                    }
                }
            }

            let parsed = ParsedMessage {
                message_type: PARSE_MESSAGE,
                sql: Some(sql),
                has_embed_calls: false,
                rewritten: false,
            };
            return Some((data.to_vec(), parsed));
        }
    }

    None
}

pub fn create_query_message(sql: &str) -> Vec<u8> {
    let mut buffer = Vec::new();
    buffer.push(QUERY_MESSAGE);
    let length = 4 + sql.len() + 1;
    buffer.extend_from_slice(&(length as u32).to_be_bytes());
    buffer.extend_from_slice(sql.as_bytes());
    buffer.push(0);
    buffer
}

pub fn create_parse_message_with_rewritten_query(
    original_data: &[u8],
    query_start: usize,
    query_end: usize,
    rewritten_sql: &str,
) -> Vec<u8> {
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&original_data[..query_start]);
    buffer.extend_from_slice(rewritten_sql.as_bytes());
    buffer.extend_from_slice(&original_data[query_end..]);

    let new_length = buffer.len() - 1;
    buffer[1..5].copy_from_slice(&(new_length as u32).to_be_bytes());

    buffer
}
