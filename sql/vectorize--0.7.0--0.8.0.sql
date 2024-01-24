DROP function vectorize."table";

-- src/api.rs:15
-- vectorize::api::table
CREATE  FUNCTION vectorize."table"(
        "table" TEXT, /* &str */
        "columns" TEXT[], /* alloc::vec::Vec<alloc::string::String> */
        "job_name" TEXT, /* alloc::string::String */
        "primary_key" TEXT, /* alloc::string::String */
        "args" json DEFAULT '{}', /* pgrx::datum::json::Json */
        "schema" TEXT DEFAULT 'public', /* alloc::string::String */
        "update_col" TEXT DEFAULT 'last_updated_at', /* alloc::string::String */
        "transformer" TEXT DEFAULT 'text-embedding-ada-002', /* alloc::string::String */
        "search_alg" vectorize.SimilarityAlg DEFAULT 'pgv_cosine_similarity', /* vectorize::types::SimilarityAlg */
        "table_method" vectorize.TableMethod DEFAULT 'append', /* vectorize::types::TableMethod */
        "schedule" TEXT DEFAULT '* * * * *' /* alloc::string::String */
) RETURNS TEXT /* core::result::Result<alloc::string::String, anyhow::Error> */
STRICT
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'table_wrapper';

DROP FUNCTION vectorize."transform_embeddings";
-- src/api.rs:170
-- vectorize::api::transform_embeddings
CREATE  FUNCTION vectorize."transform_embeddings"(
        "input" TEXT, /* &str */
        "model_name" TEXT DEFAULT 'text-embedding-ada-002', /* alloc::string::String */
        "api_key" TEXT DEFAULT NULL /* core::option::Option<alloc::string::String> */
) RETURNS double precision[] /* core::result::Result<alloc::vec::Vec<f64>, pgrx::spi::SpiError> */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'transform_embeddings_wrapper';