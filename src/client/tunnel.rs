//! Authentication and opening connections over a shared tunnel.
use super::{
    ForwardConnection, ForwardProtocol, broken_pipe,
    session::{self, Command},
};
use crate::StreamLimits;
use bsobfs::codec::fake_codec::{CredentialsSide, FakeCodec, FakeCodecCfg};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpStream, ToSocketAddrs},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_util::{
    bytes::{Bytes, BytesMut},
    codec::{Decoder, Encoder, Framed},
};

/// A shared tunnel used to open independent TCP and UDP forwarding connections.
#[derive(Clone)]
pub struct ForwardClient {
    commands: mpsc::Sender<Command>,
    closes: mpsc::UnboundedSender<u64>,
    next_id: Arc<AtomicU64>,
    limits: StreamLimits,
}

impl ForwardClient {
    /// Authenticate once against a server configured with ForwardTo::Multiplexed.
    /// The task reports tunnel errors; drop every handle or call shutdown to stop it.
    ///
    /// ```no_run
    /// use bluesky_obfs_tunnel::{client::ForwardClient, StreamLimits};
    /// use bsobfs::codec::fake_codec::FakeCodecCfg;
    /// use tokio_util::bytes::Bytes;
    ///
    /// async fn forwards(cfg: FakeCodecCfg) -> Result<(), Box<dyn std::error::Error>> {
    ///     let (client, driver) = ForwardClient::connect(
    ///         cfg, "127.0.0.1:443", StreamLimits::default(),
    ///     ).await?;
    ///     let (mut tcp1, mut tcp2, mut udp) = tokio::try_join!(
    ///         client.open_tcp("127.0.0.1:8080"),
    ///         client.open_tcp("127.0.0.1:8081"),
    ///         client.open_udp("127.0.0.1:5353"),
    ///     )?;
    ///     tcp1.send(Bytes::from_static(b"request one")).await?;
    ///     tcp2.send(Bytes::from_static(b"request two")).await?;
    ///     udp.send(Bytes::from_static(b"datagram")).await?;
    ///     let _responses = tokio::try_join!(tcp1.recv(), tcp2.recv(), udp.recv())?;
    ///     client.shutdown().await?;
    ///     driver.await??;
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect<S: ToSocketAddrs>(
        cfg: FakeCodecCfg,
        server: S,
        limits: StreamLimits,
    ) -> io::Result<(Self, JoinHandle<io::Result<()>>)> {
        if !matches!(cfg.credentials, CredentialsSide::Client(_)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "client credentials are required",
            ));
        }
        let mut stream =
            tokio::time::timeout(cfg.handshake_timeout, TcpStream::connect(server)).await??;
        stream.set_nodelay(true)?;
        let mut codec = FakeCodec::new(cfg);
        if !codec.setup_stream(&mut stream).await {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "tunnel authentication failed",
            ));
        }
        Ok(Self::from_framed(Framed::new(stream, codec), limits))
    }

    /// Start multiplexing an already authenticated framed transport.
    pub fn from_framed<C, T>(
        stream: Framed<T, C>,
        limits: StreamLimits,
    ) -> (Self, JoinHandle<io::Result<()>>)
    where
        C: Decoder<Item = BytesMut, Error = io::Error>
            + Encoder<Bytes, Error = io::Error>
            + Unpin
            + Send
            + 'static,
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (commands, receiver) = mpsc::channel(limits.sink_queue_depth.max(1));
        // Only connection IDs use this unbounded channel, so Drop can always close
        // a connection even while the bounded data queue is full.
        let (closes, closed) = mpsc::unbounded_channel();
        let client = Self {
            commands,
            closes,
            next_id: Arc::new(AtomicU64::new(1)),
            limits,
        };
        let task = tokio::spawn(session::run(stream, receiver, closed, limits));
        (client, task)
    }

    pub async fn open_tcp(&self, destination: impl Into<String>) -> io::Result<ForwardConnection> {
        self.open(destination.into(), ForwardProtocol::Tcp).await
    }

    pub async fn open_udp(&self, destination: impl Into<String>) -> io::Result<ForwardConnection> {
        self.open(destination.into(), ForwardProtocol::Udp).await
    }

    async fn open(
        &self,
        destination: String,
        protocol: ForwardProtocol,
    ) -> io::Result<ForwardConnection> {
        let id = self
            .next_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| io::Error::other("connection IDs exhausted"))?;
        let (incoming, receiver) = mpsc::channel(self.limits.max_pending_frames.max(1));
        let (error_tx, error_rx) = watch::channel(None);
        // Construct the handle before awaiting: cancelling an open also closes its
        // in-flight server connect, even if the acknowledgement races cancellation.
        let connection = ForwardConnection::new(
            id,
            protocol,
            self.commands.clone(),
            self.closes.clone(),
            receiver,
            error_rx,
        );
        let (ready, result) = oneshot::channel();
        self.commands
            .send(Command::Open {
                id,
                protocol,
                destination,
                incoming,
                error: error_tx,
                ready,
            })
            .await
            .map_err(|_| broken_pipe())?;
        result.await.map_err(|_| broken_pipe())??;
        Ok(connection)
    }

    /// Close every logical connection and finish writing the tunnel.
    pub async fn shutdown(&self) -> io::Result<()> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(Command::Stop(reply))
            .await
            .map_err(|_| broken_pipe())?;
        response.await.map_err(|_| broken_pipe())?
    }
}
