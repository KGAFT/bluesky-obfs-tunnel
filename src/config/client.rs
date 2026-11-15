use super::{
    ClientCredential, ObfuscationConfig, PatternConfig, QueueConfig, ReconnectConfig,
    validation::{host_port, nonempty, positive, result},
};
use crate::StreamLimits;
use bsobfs::{
    codec::fake_codec::{CredentialsSide, FakeCodecCfg},
    obfs_data::strategy::ConnectionPattern,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ClientSettings {
    pub server_address: String,
    /// Used by --http-proxy when no listener address is supplied on the CLI.
    pub local_proxy_bind: String,
    pub client_identity: String,
    pub client_password: String,
    pub proxy_setup_timeout_secs: u64,
    /// Maximum concurrent accepted HTTP proxy connections.
    pub proxy_setup_queue_depth: u32,
    pub queues: QueueConfig,
    pub obfuscation: ObfuscationConfig,
    pub patterns: PatternConfig,
    pub reconnect: ReconnectConfig,
}

impl Default for ClientSettings {
    fn default() -> Self {
        let credential = ClientCredential::default();
        Self {
            server_address: "127.0.0.1:443".into(),
            local_proxy_bind: "127.0.0.1:8080".into(),
            client_identity: credential.identity,
            client_password: credential.password,
            proxy_setup_timeout_secs: 30,
            proxy_setup_queue_depth: 256,
            queues: QueueConfig::default(),
            obfuscation: ObfuscationConfig::default(),
            patterns: PatternConfig::default(),
            reconnect: ReconnectConfig::default(),
        }
    }
}

impl ClientSettings {
    pub fn limits(&self) -> StreamLimits {
        self.queues.limits()
    }

    pub fn proxy_setup_timeout(&self) -> Duration {
        Duration::from_secs(self.proxy_setup_timeout_secs)
    }

    pub fn credentials(&self) -> CredentialsSide {
        CredentialsSide::Client(Arc::new(ClientCredential {
            identity: self.client_identity.clone(),
            password: self.client_password.clone(),
        }))
    }

    pub fn codec_cfg(
        &self,
        client_pattern: ConnectionPattern,
        credentials: CredentialsSide,
    ) -> FakeCodecCfg {
        self.obfuscation
            .to_codec_cfg(client_pattern, credentials, None, None)
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        // Unlike a TUN client, this client can use the system resolver normally.
        if !host_port(&self.server_address) {
            problems.push("server_address must be host:port (IPv6 must use brackets)".into());
        }
        if self.local_proxy_bind.parse::<SocketAddr>().is_err() {
            problems.push("local_proxy_bind must be ip:port".into());
        }
        nonempty("client_identity", &self.client_identity, &mut problems);
        nonempty("client_password", &self.client_password, &mut problems);
        positive(
            "proxy_setup_timeout_secs",
            self.proxy_setup_timeout_secs,
            &mut problems,
        );
        positive(
            "proxy_setup_queue_depth",
            self.proxy_setup_queue_depth.into(),
            &mut problems,
        );
        self.queues.validate(&mut problems);
        self.obfuscation.validate(&mut problems);
        self.patterns.validate(&mut problems);
        if let Err(errors) = self.reconnect.validate() {
            problems.extend(errors);
        }
        result(problems)
    }
}
