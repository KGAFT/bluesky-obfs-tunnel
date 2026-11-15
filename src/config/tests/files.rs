use super::*;
use tokio::fs;

#[tokio::test]
async fn creates_defaults_and_reloads_saved_operator_settings() {
    let directory = TestDir::new();
    let path = directory.path("client.json");
    let mut client = ClientSettings::load_or_create(&path).await.unwrap();
    assert_eq!(client, ClientSettings::default());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).await.unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    client.server_address = "vpn.example.com:443".into();
    client.reconnect.session_min_secs = 60;
    client.reconnect.session_max_secs = 90;
    client.save(&path).await.unwrap();
    assert_eq!(ClientSettings::load_or_create(&path).await.unwrap(), client);
    let server_path = directory.path("server.json");
    let mut server = ServerSettings::load_or_create(&server_path).await.unwrap();
    server.forwarding = ForwardMode::Multiplexed;
    server.save(&server_path).await.unwrap();
    assert_eq!(ServerSettings::load(&server_path).await.unwrap(), server);
}

#[tokio::test]
async fn existing_malformed_or_invalid_files_are_fatal_and_preserved() {
    let directory = TestDir::new();
    let path = directory.path("settings.json");
    for contents in [
        "{ broken",
        r#"{"queues":{"sink_queue_depth":0}}"#,
        r#"{"tun_name":"tun0"}"#,
    ] {
        fs::write(&path, contents).await.unwrap();
        assert_eq!(
            ClientSettings::load_or_create(&path)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            ServerSettings::load_or_create(&path)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read_to_string(&path).await.unwrap(), contents);
    }
    let client = ClientSettings {
        client_identity: String::new(),
        ..ClientSettings::default()
    };
    assert!(client.save(&path).await.is_err());
    assert_eq!(
        fs::read_to_string(&path).await.unwrap(),
        r#"{"tun_name":"tun0"}"#
    );
    assert!(
        ClientSettings::load_or_create(directory.path("missing/client.json"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn patterns_use_configured_paths_and_fail_without_network_capture() {
    let directory = TestDir::new();
    let client_path = directory.path("client-pattern.bin");
    let server_path = directory.path("server-pattern.bin");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::copy(source.join("cli_pattern.bin"), &client_path)
        .await
        .unwrap();
    fs::copy(source.join("serv_pattern.bin"), &server_path)
        .await
        .unwrap();
    let settings = PatternConfig {
        client_pattern_path: client_path.to_str().unwrap().into(),
        server_pattern_path: server_path.to_str().unwrap().into(),
        ..PatternConfig::default()
    };
    assert!(settings.load().await.is_ok());
    fs::write(&client_path, b"invalid archive").await.unwrap();
    assert!(settings.load().await.is_err());
}
