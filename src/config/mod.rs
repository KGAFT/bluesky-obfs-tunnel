//! Validated JSON settings for the tunnel library and example binaries.
mod client;
mod credentials;
mod database;
mod file;
mod obfuscation;
mod patterns;
mod queues;
mod reconnect;
mod server;
mod validation;

pub use client::ClientSettings;
pub use credentials::ClientCredential;
pub use database::DatabaseConfig;
pub use obfuscation::{DelayConfig, ObfuscationConfig, RateLimiterConfig};
pub use patterns::PatternConfig;
pub use queues::QueueConfig;
pub use reconnect::ReconnectConfig;
pub use server::{ForwardMode, ServerSettings};

#[cfg(test)]
mod tests;
