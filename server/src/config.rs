#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: from_env_default(
                "DATABASE_URL",
                "postgresql://postgres:postgres@localhost:5432/postgres",
            ),
        }
    }
}

/// source a variable from environment - use default if not exists
pub fn from_env_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}
