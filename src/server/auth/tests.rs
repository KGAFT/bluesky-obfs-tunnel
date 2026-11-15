use super::*;

#[test]
fn credentials_fit_database_columns_and_cannot_be_empty() {
    for credential in [
        ClientCredential {
            identity: " ".into(),
            password: "secret".into(),
        },
        ClientCredential {
            identity: "a".repeat(256),
            password: "secret".into(),
        },
        ClientCredential {
            identity: "alice".into(),
            password: "".into(),
        },
        ClientCredential {
            identity: "alice".into(),
            password: "x".repeat(65_536),
        },
    ] {
        assert!(validate_credential(&credential).is_err());
    }
    assert!(validate_credential(&ClientCredential::default()).is_ok());
}

// Run against a disposable MariaDB database; rows created here are removed on success.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TEST_DATABASE_URL pointing to a disposable MariaDB database"]
async fn mariadb_credentials_reject_replays_across_connections_and_restarts() {
    let url = std::env::var("TEST_DATABASE_URL").expect("set TEST_DATABASE_URL");
    let config = DatabaseConfig::default();
    let window = 60_000;
    let store = ClientStore::connect(&url, &config, window).await.unwrap();
    let name = format!("auth-test-{:016x}", rand::random::<u64>());
    let credential = ClientCredential {
        identity: name.clone(),
        password: "secret".into(),
    };
    store.create_client(credential.clone()).await.unwrap();
    assert_eq!(
        store.create_client(credential).await.unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    let now = ReplayGuard::current_time_ms();
    for (name, timestamp) in [
        ("does-not-exist", now),
        (name.as_str(), 0),
        (name.as_str(), u64::MAX),
        (name.as_str(), now - window - 10_000),
        (name.as_str(), now + window + 10_000),
    ] {
        assert!(store.get_client_password(name, timestamp).await.is_none());
    }
    // Identity matching is case sensitive, regardless of the database's default collation.
    assert!(
        store
            .get_client_password(&name.to_uppercase(), now)
            .await
            .is_none()
    );
    assert_eq!(
        store.get_client_password(&name, now).await,
        Some(b"secret".to_vec())
    );
    assert!(store.get_client_password(&name, now).await.is_none());
    assert!(store.get_client_password(&name, now - 1).await.is_none());

    // Reconnect with an entirely new pool, including re-running the migration check.
    drop(store);
    let first = ClientStore::connect(&url, &config, window).await.unwrap();
    let second = ClientStore::connect(&url, &config, window).await.unwrap();
    assert!(second.get_client_password(&name, now).await.is_none());
    let timestamp = ReplayGuard::current_time_ms().max(now + 1);
    let mut attempts = Vec::new();
    for index in 0..16 {
        let store = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        let name = name.clone();
        attempts.push(tokio::spawn(async move {
            store.get_client_password(&name, timestamp).await
        }));
    }
    let mut accepted = 0;
    for attempt in attempts {
        if attempt.await.unwrap().is_some() {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 1, "only one concurrent reservation may succeed");
    assert!(first.get_client_password(&name, timestamp).await.is_none());
    assert!(
        first
            .get_client_password(&name, timestamp + 1)
            .await
            .is_some()
    );
    assert!(first.get_client_password(&name, timestamp).await.is_none());
    let pool = first.pool.clone();
    tokio::task::spawn_blocking(move || {
        diesel::delete(clients.find(name.as_bytes()))
            .execute(&mut pool.get().unwrap())
            .unwrap();
    })
    .await
    .unwrap();
}
