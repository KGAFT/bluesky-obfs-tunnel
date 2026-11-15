use super::{
    ObfuscationConfig,
    validation::{host_port, nonempty},
};
use bsobfs::{
    obfs_data::pattern_store::{PatternPair, PatternStore},
    wreq::IntoEmulation,
};
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PatternConfig {
    pub client_pattern_path: String,
    pub server_pattern_path: String,
    pub capture_dest: String,
    pub capture_url: String,
}

impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            client_pattern_path: "cli_pattern.bin".into(),
            server_pattern_path: "serv_pattern.bin".into(),
            capture_dest: "www.microsoft.com:443".into(),
            capture_url: "https://www.microsoft.com/".into(),
        }
    }
}

impl PatternConfig {
    fn store(&self) -> PatternStore {
        PatternStore::new(&self.client_pattern_path, &self.server_pattern_path)
    }

    /// Load existing patterns without performing a network capture.
    pub async fn load(&self) -> io::Result<PatternPair> {
        self.store().load().await.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "cannot load pattern files '{}' and '{}'",
                    self.client_pattern_path, self.server_pattern_path
                ),
            )
        })
    }

    /// Server startup may capture missing patterns using the configured cover site.
    pub async fn load_or_capture(
        &self,
        obfuscation: &ObfuscationConfig,
    ) -> io::Result<PatternPair> {
        if let Ok(patterns) = self.load().await {
            return Ok(patterns);
        }
        for path in [&self.client_pattern_path, &self.server_pattern_path] {
            if let Some(parent) = std::path::Path::new(path)
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        let patterns = self
            .store()
            .load_or_capture(
                obfuscation.browser_emulation.into_emulation(),
                self.capture_dest.clone(),
                self.capture_url.clone(),
            )
            .await;
        self.store()
            .save(&patterns)
            .await
            .ok_or_else(|| io::Error::other("cannot save captured pattern files"))?;
        Ok(patterns)
    }

    pub(super) fn validate(&self, problems: &mut Vec<String>) {
        nonempty("client_pattern_path", &self.client_pattern_path, problems);
        nonempty("server_pattern_path", &self.server_pattern_path, problems);
        if self.client_pattern_path == self.server_pattern_path {
            problems.push("client_pattern_path and server_pattern_path must differ".into());
        }
        if !host_port(&self.capture_dest) {
            problems.push("capture_dest must be host:port".into());
        }
        if !self
            .capture_url
            .parse::<http::Uri>()
            .is_ok_and(|uri| uri.scheme_str() == Some("https") && uri.host().is_some())
        {
            problems.push("capture_url must be an absolute https:// URL".into());
        }
    }
}
