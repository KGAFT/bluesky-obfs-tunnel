use serde::{Deserialize, Serialize};

/// Connection secrets stay in the environment rather than the JSON settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url_env: String,
    pub pool_size: u32,
    pub connection_timeout_secs: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url_env: "DATABASE_URL".into(),
            pool_size: 8,
            connection_timeout_secs: 5,
        }
    }
}

impl DatabaseConfig {
    pub(super) fn validate(&self, problems: &mut Vec<String>) {
        if self.url_env.is_empty()
            || !self
                .url_env
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            problems.push("database.url_env must be an environment variable name".into());
        }
        if self.pool_size == 0 {
            problems.push("database.pool_size must be at least 1".into());
        }
        if self.connection_timeout_secs == 0 {
            problems.push("database.connection_timeout_secs must be at least 1".into());
        }
    }
}
