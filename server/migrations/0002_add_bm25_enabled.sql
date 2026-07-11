-- Opt-in flag for building/maintaining an in-memory BM25 (Tantivy) index per job.
-- Defaults to false so upgraded deployments never get BM25 indexing turned on
-- for existing jobs without an explicit opt-in via POST /table.
ALTER TABLE vectorize.job ADD COLUMN IF NOT EXISTS bm25_enabled BOOLEAN NOT NULL DEFAULT false;
