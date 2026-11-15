//! Logical TCP/UDP connections and independently usable send/receive halves.
use super::{ForwardProtocol, broken_pipe, session::Command};
use crate::protocol::MAX_TUNNEL_CHUNK;
use std::{io, sync::Arc};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::bytes::Bytes;

struct ConnectionHandle {
    id: u64,
    protocol: ForwardProtocol,
    commands: mpsc::Sender<Command>,
    closes: mpsc::UnboundedSender<u64>,
}

impl Drop for ConnectionHandle {
    fn drop(&mut self) {
        let _ = self.closes.send(self.id);
    }
}

/// TCP uses byte chunks; UDP uses complete datagrams, including empty ones.
/// Dropping this handle (or both split halves) closes only this connection.
pub struct ForwardConnection {
    sender: ForwardSender,
    receiver: ForwardReceiver,
}

impl std::fmt::Debug for ForwardConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ForwardConnection")
            .field("id", &self.id())
            .field("protocol", &self.protocol())
            .finish_non_exhaustive()
    }
}

impl ForwardConnection {
    pub(super) fn new(
        id: u64,
        protocol: ForwardProtocol,
        commands: mpsc::Sender<Command>,
        closes: mpsc::UnboundedSender<u64>,
        incoming: mpsc::Receiver<Bytes>,
        error: watch::Receiver<Option<Arc<io::Error>>>,
    ) -> Self {
        let handle = Arc::new(ConnectionHandle {
            id,
            protocol,
            commands,
            closes,
        });
        Self {
            sender: ForwardSender {
                handle: handle.clone(),
            },
            receiver: ForwardReceiver {
                incoming,
                error,
                ended: false,
                _handle: handle,
            },
        }
    }

    pub fn id(&self) -> u64 {
        self.sender.handle.id
    }
    pub fn protocol(&self) -> ForwardProtocol {
        self.sender.handle.protocol
    }
    pub async fn send(&self, data: impl Into<Bytes>) -> io::Result<()> {
        self.sender.send(data).await
    }
    pub async fn recv(&mut self) -> io::Result<Option<Bytes>> {
        self.receiver.recv().await
    }
    pub async fn shutdown_write(&self) -> io::Result<()> {
        self.sender.shutdown_write().await
    }
    pub async fn close(self) -> io::Result<()> {
        self.sender.close().await
    }
    pub fn split(self) -> (ForwardSender, ForwardReceiver) {
        (self.sender, self.receiver)
    }
}

#[derive(Clone)]
pub struct ForwardSender {
    handle: Arc<ConnectionHandle>,
}

impl ForwardSender {
    /// Completion means the data entered the tunnel writer queue, not that the
    /// remote application consumed it. Large TCP sends are split into frames.
    pub async fn send(&self, data: impl Into<Bytes>) -> io::Result<()> {
        let data = data.into();
        if self.handle.protocol == ForwardProtocol::Udp && data.len() > MAX_TUNNEL_CHUNK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "UDP datagram exceeds 8192 bytes",
            ));
        }
        let (reply, response) = oneshot::channel();
        self.handle
            .commands
            .send(Command::Data {
                id: self.handle.id,
                data,
                reply,
            })
            .await
            .map_err(|_| broken_pipe())?;
        response.await.map_err(|_| broken_pipe())?
    }

    pub async fn shutdown_write(&self) -> io::Result<()> {
        if self.handle.protocol != ForwardProtocol::Tcp {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "UDP connections have no write half-close",
            ));
        }
        let (reply, response) = oneshot::channel();
        self.handle
            .commands
            .send(Command::ShutdownWrite {
                id: self.handle.id,
                reply,
            })
            .await
            .map_err(|_| broken_pipe())?;
        response.await.map_err(|_| broken_pipe())?
    }

    pub async fn close(&self) -> io::Result<()> {
        let (reply, response) = oneshot::channel();
        self.handle
            .commands
            .send(Command::Close {
                id: self.handle.id,
                reply,
            })
            .await
            .map_err(|_| broken_pipe())?;
        response.await.map_err(|_| broken_pipe())?
    }
}

pub struct ForwardReceiver {
    incoming: mpsc::Receiver<Bytes>,
    error: watch::Receiver<Option<Arc<io::Error>>>,
    ended: bool,
    _handle: Arc<ConnectionHandle>,
}

impl ForwardReceiver {
    /// None means read EOF. Some(empty) is a valid empty UDP datagram.
    pub async fn recv(&mut self) -> io::Result<Option<Bytes>> {
        if self.ended {
            return Ok(None);
        }
        if let Some(data) = self.incoming.recv().await {
            return Ok(Some(data));
        }
        self.ended = true;
        if let Some(error) = self.error.borrow().as_ref() {
            return Err(io::Error::new(error.kind(), error.to_string()));
        }
        Ok(None)
    }
}
