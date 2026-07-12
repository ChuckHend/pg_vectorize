use sqlx::migrate::MigrateError;
use sqlx::{ConnectOptions, PgPool};

/// Applies pending sqlx migrations for the tables vectorize-server owns
/// Runs on a dedicated, non-pooled connection with `search_path` set to
/// `vectorize`, so sqlx's bookkeeping table (`_sqlx_migrations`) is created
/// inside the `vectorize` schema instead of `public`. This keeps it isolated
/// from the pool's connections, whose search_path is left untouched.
pub async fn run_migrations(pool: &PgPool) -> Result<(), MigrateError> {
    let connect_options = pool
        .connect_options()
        .as_ref()
        .clone()
        .options([("search_path", "vectorize")]);

    let mut conn = connect_options.connect().await?;
    sqlx::migrate!("./migrations").run(&mut conn).await
}
