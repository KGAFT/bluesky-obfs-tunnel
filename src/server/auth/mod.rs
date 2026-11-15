//! MariaDB credentials and durable, serialized authentication timestamp reservations.
mod schema;

use crate::config::{ClientCredential, DatabaseConfig};
use bsobfs::{
    codec::fake_codec::{CredentialsSide, ServerCredentialProvider},
    obfs_data::replay_guard::ReplayGuard,
    the_fourth_utils::async_trait::async_trait,
};
use diesel::{
    mysql::MysqlConnection,
    prelude::*,
    r2d2::{ConnectionManager, Pool},
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use schema::clients::dsl::*;
use std::{io, sync::Arc, time::Duration};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");
type ConnectionPool = Pool<ConnectionManager<MysqlConnection>>;

#[derive(Clone)]
pub struct ClientStore {
    pool: ConnectionPool,
    window_ms: u64,
}

impl ClientStore {
    pub async fn connect_env(config: &DatabaseConfig, window_ms: u64) -> io::Result<Self> {
        let url = std::env::var(&config.url_env)
            .map_err(|_| invalid(format!("set {} to a MariaDB mysql:// URL", config.url_env)))?;
        Self::connect(&url, config, window_ms).await
    }

    /// Connect and apply embedded Diesel migrations before serving clients.
    pub async fn connect(url: &str, config: &DatabaseConfig, window_ms: u64) -> io::Result<Self> {
        if config.pool_size == 0
            || config.connection_timeout_secs == 0
            || window_ms == 0
            || window_ms > i64::MAX as u64
        {
            return Err(invalid(
                "database pool, timeout and replay window must be positive",
            ));
        }
        let manager = ConnectionManager::<MysqlConnection>::new(url);
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            let pool = Pool::builder()
                .max_size(config.pool_size)
                .connection_timeout(Duration::from_secs(config.connection_timeout_secs))
                .build(manager)
                .map_err(|_| io::Error::other("cannot connect to MariaDB"))?;
            pool.get()
                .map_err(|_| io::Error::other("cannot acquire MariaDB connection"))?
                .run_pending_migrations(MIGRATIONS)
                .map_err(|error| io::Error::other(format!("database migration failed: {error}")))?;
            Ok(Self { pool, window_ms })
        })
        .await
        .map_err(io::Error::other)?
    }

    pub fn credentials(&self) -> CredentialsSide {
        CredentialsSide::Server(Arc::new(self.clone()))
    }

    /// Insert only: an existing identity is never overwritten or its replay history reset.
    pub async fn create_client(&self, credential: ClientCredential) -> io::Result<()> {
        validate_credential(&credential)?;
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = pool.get().map_err(io::Error::other)?;
            diesel::insert_into(clients)
                .values((
                    identity.eq(credential.identity.as_bytes()),
                    password.eq(credential.password.as_bytes()),
                ))
                .execute(&mut connection)
                .map_err(|error| match error {
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        _,
                    ) => io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "client identity already exists",
                    ),
                    _ => io::Error::other("cannot create client in MariaDB"),
                })?;
            Ok(())
        })
        .await
        .map_err(io::Error::other)?
    }

    fn reserve_password(
        &self,
        client_identity: &[u8],
        time_ms: u64,
    ) -> io::Result<Option<Vec<u8>>> {
        let Ok(time) = i64::try_from(time_ms) else {
            return Ok(None);
        };
        if time == 0 {
            return Ok(None);
        }
        let mut connection = self.pool.get().map_err(io::Error::other)?;
        connection
            .transaction::<_, diesel::result::Error, _>(|connection| {
                let row = clients
                    .find(client_identity)
                    .select((password, last_auth_ms))
                    .for_update()
                    .first::<(Vec<u8>, Option<i64>)>(connection)
                    .optional()?;
                let Some((secret, last)) = row else {
                    return Ok(None);
                };
                // Check freshness after acquiring the row lock: waiting must not admit stale hellos.
                if ReplayGuard::current_time_ms().abs_diff(time_ms) > self.window_ms
                    || last.is_some_and(|last| time <= last)
                {
                    return Ok(None);
                }
                diesel::update(clients.find(client_identity))
                    .set(last_auth_ms.eq(time))
                    .execute(connection)?;
                Ok(Some(secret))
            })
            .map_err(io::Error::other)
    }
}

#[async_trait]
impl ServerCredentialProvider for ClientStore {
    async fn get_client_password(&self, client_identity: &str, time_ms: u64) -> Option<Vec<u8>> {
        if client_identity.is_empty()
            || client_identity.len() > 255
            || ReplayGuard::current_time_ms().abs_diff(time_ms) > self.window_ms
        {
            return None;
        }
        let store = self.clone();
        let client_identity = client_identity.as_bytes().to_vec();
        match tokio::task::spawn_blocking(move || store.reserve_password(&client_identity, time_ms))
            .await
        {
            Ok(Ok(secret)) => secret,
            // A database failure must never fall back to accepting a client.
            _ => {
                eprintln!("authentication database unavailable; rejecting authentication");
                None
            }
        }
    }
}

fn validate_credential(credential: &ClientCredential) -> io::Result<()> {
    if credential.identity.trim().is_empty() || credential.identity.len() > 255 {
        return Err(invalid("client identity must contain 1–255 UTF-8 bytes"));
    }
    if credential.password.trim().is_empty() || credential.password.len() > 65_535 {
        return Err(invalid("client password must contain 1–65535 UTF-8 bytes"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests;
