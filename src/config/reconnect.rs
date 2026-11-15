use super::validation::{positive, result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ReconnectConfig {
    /// Disable scheduled rotation; failed tunnels still retry.
    pub enabled: bool,
    /// Inclusive lifetime range, sampled again after each successful handshake.
    pub session_min_secs: u32,
    pub session_max_secs: u32,
    pub retry_delay_secs: u32,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            session_min_secs: 30 * 60,
            session_max_secs: 40 * 60,
            retry_delay_secs: 5,
        }
    }
}

impl ReconnectConfig {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        positive(
            "session_min_secs",
            self.session_min_secs.into(),
            &mut problems,
        );
        if self.session_max_secs < self.session_min_secs {
            problems.push("session_max_secs must be at least session_min_secs".into());
        }
        positive(
            "retry_delay_secs",
            self.retry_delay_secs.into(),
            &mut problems,
        );
        result(problems)
    }

    pub fn next_lifetime(&self) -> Result<Option<Duration>, Vec<String>> {
        self.validate()?;
        Ok(self.enabled.then(|| {
            Duration::from_secs(u64::from(rand::random_range(
                self.session_min_secs..=self.session_max_secs,
            )))
        }))
    }

    pub fn retry_delay(&self) -> Duration {
        Duration::from_secs(self.retry_delay_secs.into())
    }
}
