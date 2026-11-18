//! Runs a one-shot request or persistent, reconnecting listeners.
use super::{
    config::client_config,
    http_proxy, invalid,
    options::Options,
    tcp_forward,
    transfer::{multiplexed, raw_tcp},
    udp_forward,
};
use bluesky_obfs_tunnel::{
    client::{ForwardClient, ForwardProtocol, reconnect},
    config::ClientSettings,
    protocol::MAX_TUNNEL_CHUNK,
};
use bsobfs::codec::fake_codec::FakeCodec;
use std::{io, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio_util::{bytes::Bytes, codec::Framed};

pub(super) async fn run(options: Options, settings: ClientSettings) -> io::Result<()> {
    let payload = match &options.file {
        Some(path) => Bytes::from(tokio::fs::read(path).await?),
        None => Bytes::from(
            options
                .message
                .unwrap_or_else(|| "hello from client\n".into()),
        ),
    };
    if matches!(options.destination, Some((ForwardProtocol::Udp, _)))
        && payload.len() > MAX_TUNNEL_CHUNK
    {
        return Err(invalid("UDP payload exceeds 8192 bytes"));
    }
    let cfg = client_config(&settings).await?;
    let mut output = tokio::io::stdout();
    eprintln!("Connecting to {}", settings.server_address);
    if let Some((protocol, destination)) = options.destination {
        let (client, driver) =
            ForwardClient::connect(cfg, &settings.server_address, settings.limits()).await?;
        let mut driver = AbortDriver(driver);
        let connection = match protocol {
            ForwardProtocol::Tcp => client.open_tcp(destination).await?,
            ForwardProtocol::Udp => client.open_udp(destination).await?,
        };
        multiplexed(connection, payload, &mut output).await?;
        client.shutdown().await?;
        (&mut driver.0).await.map_err(io::Error::other)?
    } else {
        let mut socket = TcpStream::connect(&settings.server_address).await?;
        socket.set_nodelay(true)?;
        let mut codec = FakeCodec::new(cfg);
        if !codec.setup_stream(&mut socket).await {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "tunnel authentication failed",
            ));
        }
        raw_tcp(Framed::new(socket, codec), payload, &mut output).await
    }
}

pub(super) async fn run_proxy(settings: ClientSettings) -> io::Result<()> {
    let listener = TcpListener::bind(&settings.local_proxy_bind).await?;
    let cfg = client_config(&settings).await?;
    eprintln!(
        "HTTP proxy listening on http://{} (Ctrl-C to stop)",
        listener.local_addr()?
    );
    tokio::select! {
        result = reconnect::supervise(
            &settings.reconnect,
            settings.proxy_setup_timeout(),
            || ForwardClient::connect(cfg.clone(), &settings.server_address, settings.limits()),
            |client| http_proxy::serve(&listener, client, settings.proxy_setup_timeout(), settings.proxy_setup_queue_depth as usize),
        ) => result,
        signal = tokio::signal::ctrl_c() => signal,
    }
}

pub(super) async fn run_forward(
    settings: ClientSettings,
    protocol: ForwardProtocol,
    bind: SocketAddr,
    destination: String,
) -> io::Result<()> {
    match protocol {
        ForwardProtocol::Tcp => run_tcp_forward(settings, bind, destination).await,
        ForwardProtocol::Udp => run_udp_forward(settings, bind, destination).await,
    }
}

async fn run_tcp_forward(
    settings: ClientSettings,
    bind: SocketAddr,
    destination: String,
) -> io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let cfg = client_config(&settings).await?;
    eprintln!(
        "TCP forwarding listening on {} -> {} via {} (Ctrl-C to stop)",
        listener.local_addr()?,
        destination,
        settings.server_address
    );
    tokio::select! {
        result = reconnect::supervise(
            &settings.reconnect,
            settings.proxy_setup_timeout(),
            || ForwardClient::connect(cfg.clone(), &settings.server_address, settings.limits()),
            |client| tcp_forward::serve(
                &listener, client, &destination, settings.proxy_setup_timeout(),
                settings.proxy_setup_queue_depth as usize,
            ),
        ) => result,
        signal = tokio::signal::ctrl_c() => signal,
    }
}

async fn run_udp_forward(
    settings: ClientSettings,
    bind: SocketAddr,
    destination: String,
) -> io::Result<()> {
    let socket = Arc::new(UdpSocket::bind(bind).await?);
    let cfg = client_config(&settings).await?;
    eprintln!(
        "UDP forwarding listening on {} -> {} via {} (Ctrl-C to stop)",
        socket.local_addr()?,
        destination,
        settings.server_address
    );
    tokio::select! {
        result = reconnect::supervise(
            &settings.reconnect,
            settings.proxy_setup_timeout(),
            || ForwardClient::connect(cfg.clone(), &settings.server_address, settings.limits()),
            |client| udp_forward::serve(
                &socket, client, &destination, settings.proxy_setup_timeout(),
                settings.proxy_setup_queue_depth as usize,
                settings.limits().max_pending_frames,
            ),
        ) => result,
        signal = tokio::signal::ctrl_c() => signal,
    }
}

// A timeout drops run(), so the driver needs an abort-on-drop owner.
struct AbortDriver(tokio::task::JoinHandle<io::Result<()>>);

impl Drop for AbortDriver {
    fn drop(&mut self) {
        self.0.abort();
    }
}
