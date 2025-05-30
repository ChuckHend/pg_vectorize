use anyhow::Result;
use pgrx::spi::SpiTupleTable;
use pgrx::*;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Pool, Postgres};
use std::env;
use url::{ParseError, Url};
use vectorize_core::core::guc::VectorizeGuc;

use crate::guc;
use vectorize_core::core::types::{self, Model};

#[derive(Clone, Debug)]
pub struct Config {
    pub pg_conn_str: String,
    pub vectorize_socket_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pg_conn_str: from_env_default(
                "DATABASE_URL",
                "postgresql://postgres:postgres@localhost:5432/postgres",
            ),
            vectorize_socket_url: env::var("VECTORIZE_SOCKET_URL").ok(),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct PostgresSocketConnection {
    user: Option<String>,
    dbname: Option<String>,
    host: Option<String>,
    password: Option<String>,
    port: Option<u16>, // Add other potential query parameters as needed
}

impl PostgresSocketConnection {
    fn from_unix_socket_string(s: &str) -> Option<Self> {
        let parsed_url = url::Url::parse(s).ok()?;
        let mut connection = PostgresSocketConnection::default();

        for (key, value) in parsed_url.query_pairs() {
            match key.as_ref() {
                "user" => connection.user = Some(value.into_owned()),
                "dbname" => connection.dbname = Some(value.into_owned()),
                "host" => connection.host = Some(value.into_owned()),
                "password" => connection.password = Some(value.into_owned()),
                "port" => connection.port = Some(value.parse::<u16>().expect("invalid port")),
                // Add other potential query parameters as needed
                _ => {} // Ignoring unknown parameters
            }
        }

        Some(connection)
    }
}

/// source a variable from environment - use default if not exists
pub fn from_env_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

pub fn get_vectorize_meta_spi(job_name: &str) -> Result<types::VectorizeMeta> {
    let query: &str = "
        SELECT 
            job_id,
            name,
            index_dist_type,
            transformer,
            params
        FROM vectorize.job
        WHERE name = $1
    ";
    let result: Result<types::VectorizeMeta> = Spi::connect(|client| {
        let tup_table: SpiTupleTable = client.select(query, Some(1), &[job_name.into()])?;
        if tup_table.is_empty() {
            return Err(anyhow::anyhow!(
                "project '{}' not yet initialized. Please initialize the project.",
                job_name
            ));
        }

        let result_row = tup_table.first();
        let job_id: i64 = result_row
            .get_by_name("job_id")
            .expect("job_id column does not exist.")
            .expect("job_id column was null.");
        let name: String = result_row
            .get_by_name("name")
            .expect("name column does not exist.")
            .expect("name column was null.");
        let index_dist_type: String = result_row
            .get_by_name("index_dist_type")
            .expect("index_dist_type column does not exist.")
            .expect("index_dist_type column was null.");
        let transformer: String = result_row
            .get_by_name("transformer")
            .expect("transformer column does not exist.")
            .expect("transformer column was null.");
        let params: pgrx::JsonB = result_row
            .get_by_name("params")
            .expect("params column does not exist.")
            .expect("params column was null.");

        let transformer_model = Model::new(&transformer)?;
        Ok(types::VectorizeMeta {
            job_id,
            name,
            index_dist_type: index_dist_type.into(),
            transformer: transformer_model,
            params: serde_json::to_value(params).unwrap(),
        })
    });
    result
}

pub async fn get_pg_conn() -> Result<Pool<Postgres>> {
    let mut cfg = Config::default();

    if let Some(host) = guc::get_guc(VectorizeGuc::Host) {
        info!("Using socket url from GUC: {:?}", host);
        cfg.vectorize_socket_url = Some(host);
    };

    let mut opts = get_pg_options(cfg)?;

    if let Some(dbname) = guc::get_guc(VectorizeGuc::DatabaseName) {
        opts = opts.database(&dbname)
    };

    opts = opts.application_name("pg-vectorize");

    let pgp = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(4))
        .max_connections(4)
        .connect_with(opts)
        .await?;
    Ok(pgp)
}

pub fn get_pgc_socket_opt(socket_conn: PostgresSocketConnection) -> Result<PgConnectOptions> {
    let mut opts = PgConnectOptions::new();
    opts = opts.socket(socket_conn.host.expect("missing socket host"));
    if socket_conn.port.is_some() {
        opts = opts.port(socket_conn.port.expect("missing socket port"));
    } else {
        opts = opts.port(5432);
    }
    if socket_conn.dbname.is_some() {
        opts = opts.database(&socket_conn.dbname.expect("missing socket dbname"));
    } else {
        opts = opts.database("postgres");
    }
    if socket_conn.user.is_some() {
        opts = opts.username(&socket_conn.user.expect("missing socket user"));
    } else {
        opts = opts.username("postgres");
    }
    if socket_conn.password.is_some() {
        opts = opts.password(&socket_conn.password.expect("missing socket password"));
    }
    Ok(opts)
}

fn get_pgc_tcp_opt(url: Url) -> Result<PgConnectOptions> {
    let options = PgConnectOptions::new()
        .host(url.host_str().ok_or(ParseError::EmptyHost)?)
        .port(url.port().ok_or(ParseError::InvalidPort)?)
        .username(url.username())
        .password(url.password().ok_or(ParseError::IdnaError)?)
        .database(url.path().trim_start_matches('/'));
    info!("tcp options: {:?}", options);
    Ok(options)
}

pub fn get_pg_options(cfg: Config) -> Result<PgConnectOptions> {
    match cfg.vectorize_socket_url {
        Some(socket_url) => {
            info!("VECTORIZE_SOCKET_URL={:?}", socket_url);
            let socket_conn = PostgresSocketConnection::from_unix_socket_string(&socket_url)
                .expect("failed to parse socket url");
            get_pgc_socket_opt(socket_conn)
        }
        None => {
            info!("DATABASE_URL={}", cfg.pg_conn_str);
            let url = Url::parse(&cfg.pg_conn_str)?;
            get_pgc_tcp_opt(url)
        }
    }
}

pub async fn ready(conn: &Pool<Postgres>) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1
            FROM pg_tables
            WHERE schemaname = 'vectorize'
        ) AND EXISTS (
            SELECT 1
            FROM pg_tables
            WHERE schemaname = 'pgmq'
            AND tablename = 'q_vectorize_jobs'
        ) AS both_exist;",
    )
    .fetch_one(conn)
    .await
    .expect("failed")
}
