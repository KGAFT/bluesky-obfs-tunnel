use crate::server::{
    StreamLimits, client_session::ClientSession, connect_policy::ConnectPolicy,
    tcp_connection::TcpConnection, udp_connection::UdpConnection,
};
use bsobfs::codec::fake_codec::{FakeCodec, FakeCodecCfg};
use std::{io, sync::Arc};
use tokio::net::{TcpListener, ToSocketAddrs};
use tokio_util::codec::Framed;

pub enum ForwardTo {
    /// Raw stream bytes inside the obfuscated frames.
    Tcp(TcpConnection),
    /// Each obfuscated frame contains exactly one datagram.
    Udp(UdpConnection),
    /// Multiplexed Connect/HttpPacket frames, compatible with bluesky clients.
    HttpProxy(ConnectPolicy),
    /// Multiple independent TCP and UDP destinations in one client tunnel.
    Multiplexed(ConnectPolicy),
}

pub struct ForwardServer {
    listener: TcpListener,
    target: Arc<ForwardTo>,
    cfg: FakeCodecCfg,
    limits: StreamLimits,
}

impl ForwardServer {
    pub async fn new<S: ToSocketAddrs>(
        cfg: FakeCodecCfg,
        forward_to: ForwardTo,
        bind: S,
    ) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(bind).await?,
            target: Arc::new(forward_to),
            cfg,
            limits: StreamLimits::default(),
        })
    }

    pub fn with_limits(mut self, limits: StreamLimits) -> Self {
        self.limits = limits;
        self
    }

    pub async fn start(self) -> io::Result<()> {
        let Self {
            listener,
            target,
            cfg,
            limits,
        } = self;
        loop {
            let (mut client, peer) = listener.accept().await?;
            let target = Arc::clone(&target);
            let cfg = cfg.clone();
            tokio::spawn(async move {
                let mut codec = FakeCodec::new(cfg);
                if !codec.setup_stream(&mut client).await {
                    eprintln!("Handshake failed for {peer}");
                    return;
                }
                let stream = Framed::new(client, codec);
                if let Err(e) = ClientSession::new(stream, target, limits).run().await {
                    eprintln!("Forwarding error for {peer}: {e}");
                }
            });
        }
    }
}
