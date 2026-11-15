//! Load captured patterns and construct the configured client codec.
use bluesky_obfs_tunnel::config::ClientSettings;
use bsobfs::codec::fake_codec::FakeCodecCfg;
use std::io;

pub(super) async fn client_config(settings: &ClientSettings) -> io::Result<FakeCodecCfg> {
    let patterns = settings.patterns.load().await?;
    Ok(settings.codec_cfg(patterns.client, settings.credentials()))
}
