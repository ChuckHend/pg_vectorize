use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use sqlx::Row;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, STORED, STRING, Schema, TEXT};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};
use tokio::sync::{Mutex, RwLock};
use vectorize_core::types::VectorizeJob;

pub struct BM25Index {
    index: Index,
    writer: IndexWriter,
    reader: IndexReader,
    pk_field: Field,
    body_field: Field,
}

impl BM25Index {
    pub fn new() -> Result<Self> {
        let mut schema_builder = Schema::builder();
        let pk_field = schema_builder.add_text_field("pk", STRING | STORED);
        let body_field = schema_builder.add_text_field("body", TEXT);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema);
        let writer = index.writer(50_000_000)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        Ok(BM25Index {
            index,
            writer,
            reader,
            pk_field,
            body_field,
        })
    }

    /// Delete-then-add each record so repeated calls act as upserts.
    pub fn upsert_documents(&mut self, records: &[(String, String)]) -> Result<()> {
        for (pk, text) in records {
            self.writer
                .delete_term(Term::from_field_text(self.pk_field, pk));
            let mut doc = TantivyDocument::default();
            doc.add_text(self.pk_field, pk);
            doc.add_text(self.body_field, text);
            self.writer.add_document(doc)?;
        }
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Returns PKs ordered by BM25 score descending, up to `limit`.
    /// Uses lenient parsing so malformed user queries never return an error.
    pub fn search(&self, query_text: &str, limit: usize) -> Result<Vec<String>> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.body_field]);
        let (query, _errors) = query_parser.parse_query_lenient(query_text);
        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;
            if let Some(tantivy::schema::OwnedValue::Str(pk)) = doc.get_first(self.pk_field) {
                results.push(pk.clone());
            }
        }
        Ok(results)
    }
}

/// Fetch all rows from the source table and index them into `index`.
/// Spawned as a background task after job creation so startup is non-blocking.
pub async fn populate_bm25_index(
    db_pool: &sqlx::PgPool,
    job: &VectorizeJob,
    index: Arc<Mutex<BM25Index>>,
) {
    let text_cols = job
        .src_columns
        .iter()
        .map(|c| format!("COALESCE({c}, '')"))
        .collect::<Vec<_>>()
        .join(" || ' ' || ");

    let query = format!(
        "SELECT {pk}::text as pk, {text_cols} as document_text FROM {schema}.{table}",
        pk = job.primary_key,
        schema = job.src_schema,
        table = job.src_table,
    );

    match sqlx::query(&query).fetch_all(db_pool).await {
        Ok(rows) => {
            let records: Vec<(String, String)> = rows
                .iter()
                .map(|r| (r.get("pk"), r.get("document_text")))
                .collect();
            let mut idx = index.lock().await;
            if let Err(e) = idx.upsert_documents(&records) {
                tracing::error!(
                    "BM25 initial population failed for job {}: {e}",
                    job.job_name
                );
            } else {
                tracing::info!(
                    "BM25 index populated for job {} ({} docs)",
                    job.job_name,
                    records.len()
                );
            }
        }
        Err(e) => {
            tracing::error!("BM25 population query failed for job {}: {e}", job.job_name);
        }
    }
}

/// Background task that polls `_search_tokens_{job}` for rows updated since the
/// last sync and re-indexes changed documents into the in-memory Tantivy index.
pub async fn start_bm25_sync_task(
    db_pool: sqlx::PgPool,
    bm25_indexes: Arc<RwLock<HashMap<String, Arc<Mutex<BM25Index>>>>>,
    job_cache: Arc<RwLock<HashMap<String, VectorizeJob>>>,
) {
    let mut last_synced: HashMap<String, chrono::DateTime<chrono::Utc>> = HashMap::new();

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

        let jobs: Vec<VectorizeJob> = {
            let cache = job_cache.read().await;
            cache
                .values()
                .filter(|job| job.bm25_enabled)
                .cloned()
                .collect()
        };

        for job in &jobs {
            if job.job_name.is_empty() {
                continue;
            }

            let since = last_synced
                .get(&job.job_name)
                .copied()
                .unwrap_or(chrono::DateTime::<chrono::Utc>::MIN_UTC);

            let text_cols = job
                .src_columns
                .iter()
                .map(|c| format!("COALESCE(t0.{c}, '')"))
                .collect::<Vec<_>>()
                .join(" || ' ' || ");

            let query = format!(
                "SELECT t0.{pk}::text as pk, {text_cols} as document_text
                 FROM vectorize._search_tokens_{job_name} s
                 JOIN {schema}.{table} t0 ON t0.{pk} = s.{pk}
                 WHERE s.updated_at > $1",
                pk = job.primary_key,
                job_name = job.job_name,
                schema = job.src_schema,
                table = job.src_table,
            );

            match sqlx::query(&query).bind(since).fetch_all(&db_pool).await {
                Ok(rows) if !rows.is_empty() => {
                    let records: Vec<(String, String)> = rows
                        .iter()
                        .map(|r| (r.get("pk"), r.get("document_text")))
                        .collect();

                    let indexes = bm25_indexes.read().await;
                    if let Some(idx) = indexes.get(&job.job_name) {
                        let mut idx = idx.lock().await;
                        if let Err(e) = idx.upsert_documents(&records) {
                            tracing::warn!("BM25 sync failed for job {}: {e}", job.job_name);
                        } else {
                            tracing::debug!(
                                "BM25 synced {} docs for job {}",
                                records.len(),
                                job.job_name
                            );
                        }
                    }
                    last_synced.insert(job.job_name.clone(), chrono::Utc::now());
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("BM25 sync query failed for job {}: {e}", job.job_name);
                }
            }
        }
    }
}
