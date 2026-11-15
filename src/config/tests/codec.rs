use super::*;
use bsobfs::the_fourth_utils::async_trait::async_trait;
use bsobfs::{
    codec::fake_codec::{CredentialsSide, ServerCredentialProvider},
    obfs_data::{delay_generator::DelayType, strategy::ConnectionPattern},
    the_fourth_utils::sha2::{Digest, Sha256},
};
use std::sync::Arc;

struct NoCredentials;

#[async_trait]
impl ServerCredentialProvider for NoCredentials {
    async fn get_client_password(&self, _: &str, _: u64) -> Option<Vec<u8>> {
        None
    }
}

fn credentials() -> CredentialsSide {
    CredentialsSide::Server(Arc::new(NoCredentials))
}

#[test]
fn codec_builders_use_configured_crypto_delays_and_server_guards() {
    let mut server = ServerSettings::default();
    server.obfuscation.public_password = "custom public password".into();
    server.obfuscation.server_id = "custom-server".into();
    server.obfuscation.setup_proxy_port = 8001;
    server.obfuscation.message_padding_min = 20;
    server.obfuscation.message_padding_max = 60;
    server.obfuscation.delays = vec![
        DelayConfig::ArraySort { len: 7, repeats: 3 },
        DelayConfig::SpinLoop { micros: 55 },
    ];
    let codec = server.codec_cfg(
        (ConnectionPattern::new(), ConnectionPattern::new()),
        credentials(),
    );
    assert_eq!(
        codec.public_password,
        Sha256::digest("custom public password").to_vec()
    );
    assert_eq!(codec.server_id, b"custom-server");
    assert_eq!(codec.setup_proxy_port, 8001);
    assert_eq!(codec.message_padding_size, 20..60);
    assert_eq!(codec.replay_guard.as_ref().unwrap().window_ms(), 60_000);
    assert!(matches!(
        codec.allowed_delays[0],
        DelayType::ArraySort((7, 3))
    ));
    assert!(
        matches!(codec.allowed_delays[1], DelayType::SpinLoop(duration) if duration.as_micros() == 55)
    );
    assert_eq!(
        codec
            .rate_limiter
            .unwrap()
            .packet_count_max_derivation_percent,
        0.8
    );
    server.replay_guard_window_ms = 0;
    server.rate_limiter = None;
    let codec = server.codec_cfg(
        (ConnectionPattern::new(), ConnectionPattern::new()),
        credentials(),
    );
    assert!(codec.replay_guard.is_none() && codec.rate_limiter.is_none());
    let client = ClientSettings::default();
    let codec = client.codec_cfg(ConnectionPattern::new(), client.credentials());
    assert!(codec.replay_guard.is_none() && codec.rate_limiter.is_none());
}

#[tokio::test]
async fn configured_client_credentials_are_passed_to_the_codec() {
    let client = ClientSettings {
        client_identity: "alice".into(),
        client_password: "secret".into(),
        ..ClientSettings::default()
    };
    let CredentialsSide::Client(provider) = client.credentials() else {
        unreachable!()
    };
    assert_eq!(
        provider.get_client_credentials().await,
        Some((b"alice".to_vec(), b"secret".to_vec()))
    );
}

#[tokio::test]
async fn policy_refuses_public_addresses_including_ipv4_mapped_ipv6() {
    let server = ServerSettings {
        allow_local_proxy_connections: false,
        public_ips: vec!["216.203.21.97".into()],
        ..ServerSettings::default()
    };
    let policy = server.connect_policy().unwrap();
    for destination in [
        "216.203.21.97:3306",
        "[::ffff:216.203.21.97]:3306",
        "127.0.0.1:80",
    ] {
        assert!(policy.resolve(destination).await.is_err());
    }
    let server = ServerSettings {
        local_proxy_allowlist: vec!["127.0.0.1:80".into()],
        ..server
    };
    assert!(
        server
            .connect_policy()
            .unwrap()
            .resolve("127.0.0.1:80")
            .await
            .is_ok()
    );
}
