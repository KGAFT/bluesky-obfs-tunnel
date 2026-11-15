use super::*;
use bsobfs::wreq_util::Profile;
use std::time::Duration;

#[test]
fn shipped_examples_validate_and_use_matching_peer_settings() {
    let client: ClientSettings =
        serde_json::from_str(include_str!("../../../config/client.example.json")).unwrap();
    let server: ServerSettings =
        serde_json::from_str(include_str!("../../../config/server.example.json")).unwrap();
    client.validate().unwrap();
    server.validate().unwrap();
    assert_eq!(client, ClientSettings::default());
    assert_eq!(server.obfuscation, client.obfuscation);
    assert_eq!(server.patterns, client.patterns);
    assert_eq!(server.database, DatabaseConfig::default());
    assert_eq!(server.forwarding, ForwardMode::Multiplexed);
}

#[test]
fn defaults_round_trip_and_reject_tun_fields_or_the_wrong_side() {
    let server = ServerSettings::default();
    let client = ClientSettings::default();
    server.validate().unwrap();
    client.validate().unwrap();
    let server_json = serde_json::to_value(&server).unwrap();
    let client_json = serde_json::to_value(&client).unwrap();
    assert_eq!(
        serde_json::from_value::<ServerSettings>(server_json.clone()).unwrap(),
        server
    );
    assert_eq!(
        serde_json::from_value::<ClientSettings>(client_json.clone()).unwrap(),
        client
    );
    assert!(serde_json::from_value::<ServerSettings>(client_json).is_err());
    assert!(serde_json::from_value::<ClientSettings>(server_json).is_err());
    for json in [
        r#"{"tun_name":"tun0"}"#,
        r#"{"reconnect":{"session_mni_secs":10}}"#,
        r#"{"obfuscation":{"padding":10}}"#,
    ] {
        assert!(serde_json::from_str::<ClientSettings>(json).is_err());
    }
}

#[test]
fn partial_settings_have_defaults_and_browser_profiles_use_wire_names() {
    let client: ClientSettings = serde_json::from_str(r#"{"server_address":"vpn.example.com:443","obfuscation":{"browser_emulation":"chrome_141"}}"#).unwrap();
    client.validate().unwrap();
    assert_eq!(client.obfuscation.browser_emulation, Profile::Chrome141);
    assert_eq!(
        serde_json::to_value(ObfuscationConfig::default()).unwrap()["browser_emulation"],
        "chrome_149"
    );
    assert!(
        serde_json::from_str::<ObfuscationConfig>(r#"{"browser_emulation":"netscape"}"#).is_err()
    );
}

#[test]
fn validation_collects_problems_and_does_not_silently_drop_invalid_ips() {
    let mut settings = ServerSettings {
        bind_address: "invalid".into(),
        public_ips: vec!["bad-ip".into()],
        ..ServerSettings::default()
    };
    settings.queues.sink_queue_depth = 0;
    settings.obfuscation.message_padding_max = 1;
    let problems = settings.validate().unwrap_err();
    assert_eq!(problems.len(), 4, "{problems:?}");
    assert!(settings.connect_policy().is_err());
    settings.database.pool_size = 0;
    assert!(
        settings
            .validate()
            .unwrap_err()
            .iter()
            .any(|p| p.contains("database.pool_size"))
    );
}

#[test]
fn database_and_replay_settings_are_required_and_validated() {
    let mut server = ServerSettings::default();
    server.database.url_env = "BAD=NAME".into();
    server.database.pool_size = 0;
    server.database.connection_timeout_secs = 0;
    server.replay_guard_window_ms = 0;
    assert_eq!(server.validate().unwrap_err().len(), 4);
    // Old static server credentials cannot silently remain active.
    assert!(serde_json::from_str::<ServerSettings>(r#"{"clients":[]}"#).is_err());
}

#[test]
fn client_addresses_timeouts_and_queue_depths_are_validated() {
    for address in ["vpn.example.com:443", "127.0.0.1:9080", "[::1]:443"] {
        ClientSettings {
            server_address: address.into(),
            ..ClientSettings::default()
        }
        .validate()
        .unwrap();
    }
    for address in [
        "host:0",
        "host:65536",
        "::1:443",
        "host",
        "http://host:443",
        ":443",
    ] {
        assert!(
            ClientSettings {
                server_address: address.into(),
                ..ClientSettings::default()
            }
            .validate()
            .is_err()
        );
    }
    let mut client = ClientSettings {
        proxy_setup_timeout_secs: 0,
        proxy_setup_queue_depth: 0,
        ..ClientSettings::default()
    };
    client.obfuscation.setup_proxy_port = 0;
    assert_eq!(client.validate().unwrap_err().len(), 3);
    let queues = QueueConfig {
        endpoint_event_queue_depth: 0,
        max_pending_frames: 0,
        sink_queue_depth: 0,
    };
    let limits = queues.limits();
    assert_eq!(limits.endpoint_event_queue_depth, 1);
    assert_eq!(limits.max_pending_frames, 1);
    assert_eq!(limits.sink_queue_depth, 1);
}

#[test]
fn reconnect_bounds_are_inclusive_and_invalid_ranges_never_panic() {
    let default = ReconnectConfig::default();
    for _ in 0..100 {
        let lifetime = default.next_lifetime().unwrap().unwrap().as_secs();
        assert!((1800..=2400).contains(&lifetime));
    }
    let mut config = ReconnectConfig {
        session_min_secs: 3,
        session_max_secs: 3,
        ..default
    };
    assert_eq!(
        config.next_lifetime().unwrap(),
        Some(Duration::from_secs(3))
    );
    config.enabled = false;
    assert_eq!(config.next_lifetime().unwrap(), None);
    config.session_max_secs = 2;
    assert!(config.next_lifetime().is_err());
    config.session_min_secs = 0;
    config.retry_delay_secs = 0;
    assert_eq!(config.validate().unwrap_err().len(), 2);
}
