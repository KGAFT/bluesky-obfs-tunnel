//! Configured tunnel server entry point.
mod options;

use bluesky_obfs_tunnel::{
    config::ClientCredential,
    server::{auth::ClientStore, fwd_server::ForwardServer},
};
use options::{HELP, Options};
use std::{io, process::ExitCode};

async fn run() -> io::Result<()> {
    let Some(options) = Options::parse(std::env::args().skip(1))? else {
        print!("{HELP}");
        return Ok(());
    };
    let settings = options.load_settings().await?;
    let store =
        ClientStore::connect_env(&settings.database, settings.replay_guard_window_ms).await?;
    if let Some(identity) = options.create_client {
        let password = std::env::var("CLIENT_PASSWORD")
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "set CLIENT_PASSWORD"))?;
        store
            .create_client(ClientCredential { identity, password })
            .await?;
        eprintln!("Client created");
        return Ok(());
    }
    let patterns = settings
        .patterns
        .load_or_capture(&settings.obfuscation)
        .await?;
    let cfg = settings.codec_cfg((patterns.client, patterns.server), store.credentials());
    let target = settings.forward_to()?;
    eprintln!(
        "Tunnel server listening on {}; forwarding={:?}",
        settings.bind_address, settings.forwarding
    );
    ForwardServer::new(cfg, target, &settings.bind_address)
        .await?
        .with_limits(settings.limits())
        .start()
        .await
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("server: {error}");
            ExitCode::FAILURE
        }
    }
}
