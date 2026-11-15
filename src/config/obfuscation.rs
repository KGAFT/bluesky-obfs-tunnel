use super::validation::{host_port, nonempty, positive, ratio};
use bsobfs::{
    codec::{
        fake_codec::{CredentialsSide, FakeCodecCfg},
        fake_codec_limiter::FakeCodecRateLimiterCfg,
    },
    obfs_data::{
        delay_generator::DelayType, replay_guard::ReplayGuard, strategy::ConnectionPattern,
    },
    the_fourth_utils::sha2::{Digest, Sha256},
    wreq::IntoEmulation,
    wreq_util::Profile,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub enum DelayConfig {
    ArraySort { len: u32, repeats: u32 },
    SpinLoop { micros: u64 },
}

impl DelayConfig {
    fn to_delay_type(&self) -> DelayType {
        match *self {
            Self::ArraySort { len, repeats } => {
                DelayType::ArraySort((len as usize, repeats as usize))
            }
            Self::SpinLoop { micros } => DelayType::SpinLoop(Duration::from_micros(micros)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ObfuscationConfig {
    pub target_sni: String,
    pub target_sni_connection_dest: String,
    pub setup_proxy_port: u16,
    pub browser_emulation: Profile,
    pub public_password: String,
    pub server_id: String,
    pub message_padding_min: u32,
    pub message_padding_max: u32,
    pub max_adjusted_padding_derivation_percent: f64,
    pub long_delay_min_secs: u16,
    pub long_delay_max_secs: u16,
    pub handshake_timeout_secs: u64,
    pub delays: Vec<DelayConfig>,
}

impl Default for ObfuscationConfig {
    fn default() -> Self {
        Self {
            target_sni: "https://www.microsoft.com/".into(),
            target_sni_connection_dest: "www.microsoft.com:443".into(),
            setup_proxy_port: 7756,
            browser_emulation: Profile::Chrome149,
            public_password: "TestPassword123!".into(),
            server_id: "test-server".into(),
            message_padding_min: 12,
            message_padding_max: 50,
            max_adjusted_padding_derivation_percent: 0.8,
            long_delay_min_secs: 2,
            long_delay_max_secs: 8,
            handshake_timeout_secs: 30,
            delays: vec![
                DelayConfig::ArraySort {
                    len: 32,
                    repeats: 2,
                },
                DelayConfig::SpinLoop { micros: 150 },
                DelayConfig::ArraySort {
                    len: 22,
                    repeats: 2,
                },
            ],
        }
    }
}

impl ObfuscationConfig {
    pub fn public_key(&self) -> Vec<u8> {
        Sha256::digest(&self.public_password).to_vec()
    }

    pub(super) fn validate(&self, problems: &mut Vec<String>) {
        if self.message_padding_max <= self.message_padding_min {
            problems.push("message_padding_max must be greater than message_padding_min".into());
        }
        if self.long_delay_max_secs <= self.long_delay_min_secs {
            problems.push("long_delay_max_secs must be greater than long_delay_min_secs".into());
        }
        for (name, value) in [
            ("target_sni", &self.target_sni),
            ("public_password", &self.public_password),
            ("server_id", &self.server_id),
        ] {
            nonempty(name, value, problems);
        }
        if !host_port(&self.target_sni_connection_dest) {
            problems.push("target_sni_connection_dest must be host:port".into());
        }
        positive("setup_proxy_port", self.setup_proxy_port.into(), problems);
        positive(
            "handshake_timeout_secs",
            self.handshake_timeout_secs,
            problems,
        );
        ratio(
            "max_adjusted_padding_derivation_percent",
            self.max_adjusted_padding_derivation_percent,
            problems,
        );
        if self.delays.is_empty() {
            problems.push("delays must list at least one delay".into());
        }
    }

    pub(super) fn to_codec_cfg(
        &self,
        pattern: ConnectionPattern,
        credentials: CredentialsSide,
        rate_limiter: Option<FakeCodecRateLimiterCfg>,
        replay_guard: Option<Arc<ReplayGuard>>,
    ) -> FakeCodecCfg {
        FakeCodecCfg {
            pattern,
            credentials,
            rate_limiter,
            replay_guard,
            public_password: self.public_key(),
            target_sni: self.target_sni.clone(),
            target_sni_connection_dest: self.target_sni_connection_dest.clone(),
            setup_proxy_port: self.setup_proxy_port,
            target_browser: self.browser_emulation.into_emulation(),
            message_padding_size: self.message_padding_min as usize
                ..self.message_padding_max as usize,
            server_id: self.server_id.as_bytes().to_vec(),
            max_adjusted_padding_derivation_percent: self.max_adjusted_padding_derivation_percent,
            allowed_delays: self.delays.iter().map(DelayConfig::to_delay_type).collect(),
            long_delay_secs: self.long_delay_min_secs..self.long_delay_max_secs,
            handshake_timeout: Duration::from_secs(self.handshake_timeout_secs),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimiterConfig {
    pub bandwidth_max_derivation_percent: f64,
    pub packet_count_max_derivation_percent: f64,
    pub out_of_pattern_packets_max_ratio_percent: f64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            bandwidth_max_derivation_percent: 0.8,
            packet_count_max_derivation_percent: 0.8,
            out_of_pattern_packets_max_ratio_percent: 0.8,
        }
    }
}

impl RateLimiterConfig {
    pub fn to_codec_cfg(&self, client_pattern: ConnectionPattern) -> FakeCodecRateLimiterCfg {
        FakeCodecRateLimiterCfg {
            client_pattern,
            bandwidth_max_derivation_percent: self.bandwidth_max_derivation_percent,
            packet_count_max_derivation_percent: self.packet_count_max_derivation_percent,
            out_of_pattern_packets_max_ratio_percent: self.out_of_pattern_packets_max_ratio_percent,
            ..FakeCodecRateLimiterCfg::default()
        }
    }

    pub(super) fn validate(&self, problems: &mut Vec<String>) {
        ratio(
            "bandwidth_max_derivation_percent",
            self.bandwidth_max_derivation_percent,
            problems,
        );
        ratio(
            "packet_count_max_derivation_percent",
            self.packet_count_max_derivation_percent,
            problems,
        );
        ratio(
            "out_of_pattern_packets_max_ratio_percent",
            self.out_of_pattern_packets_max_ratio_percent,
            problems,
        );
    }
}
