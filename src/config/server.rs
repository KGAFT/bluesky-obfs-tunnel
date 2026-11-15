use super::{
    DatabaseConfig, ObfuscationConfig, PatternConfig, QueueConfig, RateLimiterConfig,
    validation::{host_port, result},
};
#[cfg(feature = "server")]
use crate::server::auth::ClientStore;
use crate::{
    StreamLimits,
    server::{
        connect_policy::ConnectPolicy, fwd_server::ForwardTo, tcp_connection::TcpConnection,
        udp_connection::UdpConnection,
    },
};
use bsobfs::{
    codec::fake_codec::{CredentialsSide, FakeCodecCfg},
    obfs_data::{replay_guard::ReplayGuard, strategy::ConnectionPattern},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ForwardMode {
    Tcp { destination: SocketAddr },
    Udp { destination: SocketAddr },
    HttpProxy,
    Multiplexed,
}

impl Default for ForwardMode {
    fn default() -> Self {
        Self::Tcp {
            destination: SocketAddr::from(([127, 0, 0, 1], 2080)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ServerSettings {
    pub bind_address: String,
    pub forwarding: ForwardMode,
    pub database: DatabaseConfig,
    pub allow_local_proxy_connections: bool,
    pub local_proxy_allowlist: Vec<String>,
    pub public_ips: Vec<String>,
    pub queues: QueueConfig,
    pub obfuscation: ObfuscationConfig,
    pub rate_limiter: Option<RateLimiterConfig>,
    pub replay_guard_window_ms: u64,
    pub patterns: PatternConfig,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:443".into(),
            forwarding: ForwardMode::default(),
            database: DatabaseConfig::default(),
            allow_local_proxy_connections: true,
            local_proxy_allowlist: Vec::new(),
            public_ips: Vec::new(),
            queues: QueueConfig::default(),
            obfuscation: ObfuscationConfig::default(),
            rate_limiter: Some(RateLimiterConfig::default()),
            replay_guard_window_ms: 60_000,
            patterns: PatternConfig::default(),
        }
    }
}

impl ServerSettings {
    pub fn limits(&self) -> StreamLimits {
        self.queues.limits()
    }

    #[cfg(feature = "server")]
    pub async fn credentials(&self) -> io::Result<CredentialsSide> {
        Ok(
            ClientStore::connect_env(&self.database, self.replay_guard_window_ms)
                .await?
                .credentials(),
        )
    }

    pub fn connect_policy(&self) -> io::Result<ConnectPolicy> {
        let public_ips: Vec<IpAddr> = self
            .public_ips
            .iter()
            .map(|ip| ip.trim().parse())
            .collect::<Result<_, _>>()
            .map_err(io::Error::other)?;
        Ok(ConnectPolicy::new(
            self.allow_local_proxy_connections,
            &self.local_proxy_allowlist,
            &public_ips,
        ))
    }

    pub fn forward_to(&self) -> io::Result<ForwardTo> {
        Ok(match self.forwarding {
            ForwardMode::Tcp { destination } => {
                ForwardTo::Tcp(TcpConnection::new(destination.ip(), destination.port()))
            }
            ForwardMode::Udp { destination } => {
                ForwardTo::Udp(UdpConnection::new(destination.ip(), destination.port()))
            }
            ForwardMode::HttpProxy => ForwardTo::HttpProxy(self.connect_policy()?),
            ForwardMode::Multiplexed => ForwardTo::Multiplexed(self.connect_policy()?),
        })
    }

    pub fn codec_cfg(
        &self,
        patterns: (ConnectionPattern, ConnectionPattern),
        credentials: CredentialsSide,
    ) -> FakeCodecCfg {
        let (client_pattern, server_pattern) = patterns;
        self.obfuscation.to_codec_cfg(
            server_pattern,
            credentials,
            self.rate_limiter
                .as_ref()
                .map(|rate| rate.to_codec_cfg(client_pattern)),
            (self.replay_guard_window_ms > 0)
                .then(|| Arc::new(ReplayGuard::new(self.replay_guard_window_ms))),
        )
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.bind_address.parse::<SocketAddr>().is_err() {
            problems.push("bind_address must be ip:port".into());
        }
        if matches!(self.forwarding, ForwardMode::Tcp { destination } | ForwardMode::Udp { destination } if destination.port() == 0)
        {
            problems.push("forwarding destination port must be at least 1".into());
        }
        self.database.validate(&mut problems);
        if self.replay_guard_window_ms == 0 || self.replay_guard_window_ms > i64::MAX as u64 {
            problems.push("replay_guard_window_ms must be between 1 and i64::MAX".into());
        }
        for ip in &self.public_ips {
            if ip.trim().parse::<IpAddr>().is_err() {
                problems.push(format!("public_ips entry '{ip}' is not an IP address"));
            }
        }
        for destination in &self.local_proxy_allowlist {
            if !host_port(destination.trim()) {
                problems.push(format!(
                    "local_proxy_allowlist entry '{destination}' must be host:port"
                ));
            }
        }
        self.queues.validate(&mut problems);
        self.obfuscation.validate(&mut problems);
        self.patterns.validate(&mut problems);
        if let Some(rate) = &self.rate_limiter {
            rate.validate(&mut problems);
        }
        result(problems)
    }
}
