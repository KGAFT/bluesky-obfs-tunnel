//! Per-connection state and forwarding protocol transitions.
use super::command::{Command, Reply};
use crate::{
    client::broken_pipe,
    protocol::{Action, ForwardProtocol, MAX_TUNNEL_CHUNK, encode_action, encode_frame},
};
use std::{collections::HashMap, io, sync::Arc};
use tokio::sync::{mpsc, watch};
use tokio_util::bytes::Bytes;

struct Connection {
    protocol: ForwardProtocol,
    incoming: Option<mpsc::Sender<Bytes>>,
    error: watch::Sender<Option<Arc<io::Error>>>,
    ready: Option<Reply>,
    write_closed: bool,
}

pub(super) struct Driver {
    connections: HashMap<u64, Connection>,
    pub(super) outgoing: Option<mpsc::Sender<Bytes>>,
    stop: Option<Reply>,
}

impl Drop for Driver {
    fn drop(&mut self) {
        for connection in self.connections.values_mut() {
            connection.error.send_replace(Some(Arc::new(broken_pipe())));
        }
    }
}

impl Driver {
    pub(super) fn new(outgoing: mpsc::Sender<Bytes>) -> Self {
        Self {
            connections: HashMap::new(),
            outgoing: Some(outgoing),
            stop: None,
        }
    }

    pub(super) fn finish(&mut self, result: &io::Result<()>) {
        if let Some(reply) = self.stop.take() {
            let _ = reply.send(
                result
                    .as_ref()
                    .map(|_| ())
                    .map_err(|e| io::Error::new(e.kind(), e.to_string())),
            );
        }
    }

    async fn send_action(&self, action: Action) -> io::Result<()> {
        let frame = encode_action(action).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "control frame too large")
        })?;
        self.outgoing
            .as_ref()
            .ok_or_else(broken_pipe)?
            .send(frame)
            .await
            .map_err(|_| broken_pipe())
    }

    pub(super) async fn close(&mut self, id: u64) -> io::Result<()> {
        if self.connections.remove(&id).is_some() {
            self.send_action(Action::Close(id)).await?;
        }
        Ok(())
    }

    fn fail(&mut self, id: u64, kind: io::ErrorKind, message: &str) {
        if let Some(mut connection) = self.connections.remove(&id) {
            connection
                .error
                .send_replace(Some(Arc::new(io::Error::new(kind, message.to_owned()))));
            if let Some(reply) = connection.ready.take() {
                let _ = reply.send(Err(io::Error::new(kind, message.to_owned())));
            }
        }
    }

    pub(super) async fn command(&mut self, command: Command) -> io::Result<bool> {
        match command {
            Command::Open {
                id,
                protocol,
                destination,
                incoming,
                error,
                ready,
            } => {
                if ready.is_closed() {
                    return Ok(false);
                }
                let action = match protocol {
                    ForwardProtocol::Tcp => Action::Connect((id, destination)),
                    ForwardProtocol::Udp => Action::ConnectUdp((id, destination)),
                };
                if let Err(error) = self.send_action(action).await {
                    let _ = ready.send(Err(error));
                    return Ok(false);
                }
                self.connections.insert(
                    id,
                    Connection {
                        protocol,
                        incoming: Some(incoming),
                        error,
                        ready: Some(ready),
                        write_closed: false,
                    },
                );
            }
            Command::Data { id, data, reply } => {
                let result = self.send_data(id, data).await;
                let _ = reply.send(result);
            }
            Command::ShutdownWrite { id, reply } => {
                let result = match self.connections.get(&id) {
                    Some(connection)
                        if connection.protocol == ForwardProtocol::Tcp
                            && !connection.write_closed =>
                    {
                        let result = self.send_action(Action::ShutdownWrite(id)).await;
                        if result.is_ok() {
                            self.connections.get_mut(&id).unwrap().write_closed = true;
                        }
                        result
                    }
                    Some(connection) if connection.protocol == ForwardProtocol::Tcp => Ok(()),
                    _ => Err(broken_pipe()),
                };
                let _ = reply.send(result);
            }
            Command::Close { id, reply } => {
                let _ = reply.send(self.close(id).await);
            }
            Command::Stop(reply) => {
                self.stop = Some(reply);
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn send_data(&self, id: u64, mut data: Bytes) -> io::Result<()> {
        let connection = self.connections.get(&id).ok_or_else(broken_pipe)?;
        if connection.write_closed || connection.ready.is_some() {
            return Err(broken_pipe());
        }
        if connection.protocol == ForwardProtocol::Udp {
            let frame = encode_frame(Action::UdpPacket(id), &data).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "UDP datagram too large")
            })?;
            self.outgoing
                .as_ref()
                .ok_or_else(broken_pipe)?
                .send(frame)
                .await
                .map_err(|_| broken_pipe())?;
        } else {
            while !data.is_empty() {
                let chunk = data.split_to(data.len().min(MAX_TUNNEL_CHUNK));
                let frame =
                    encode_frame(Action::HttpPacket(id), &chunk).expect("bounded TCP payload");
                self.outgoing
                    .as_ref()
                    .ok_or_else(broken_pipe)?
                    .send(frame)
                    .await
                    .map_err(|_| broken_pipe())?;
            }
        }
        Ok(())
    }

    pub(super) async fn received(
        &mut self,
        action: Action,
        payload: Option<Bytes>,
    ) -> io::Result<()> {
        match action {
            Action::Connected(id) => {
                let Some(connection) = self.connections.get_mut(&id) else {
                    return Ok(());
                };
                if let Some(ready) = connection.ready.take()
                    && ready.send(Ok(())).is_err()
                {
                    self.close(id).await?;
                }
            }
            Action::HttpPacket(id) | Action::UdpPacket(id) => {
                let protocol = if matches!(action, Action::UdpPacket(_)) {
                    ForwardProtocol::Udp
                } else {
                    ForwardProtocol::Tcp
                };
                let Some(connection) = self.connections.get(&id) else {
                    return Ok(());
                };
                if connection.protocol != protocol || connection.ready.is_some() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "incorrect forwarding protocol",
                    ));
                }
                let data = payload.expect("validated data frame");
                if !connection
                    .incoming
                    .as_ref()
                    .is_some_and(|sender| sender.try_send(data).is_ok())
                {
                    self.fail(
                        id,
                        io::ErrorKind::WouldBlock,
                        "forward receive queue is full or closed",
                    );
                    self.send_action(Action::Close(id)).await?;
                }
            }
            Action::ShutdownWrite(id) => {
                if let Some(connection) = self.connections.get_mut(&id) {
                    if connection.protocol != ForwardProtocol::Tcp {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "UDP half-close"));
                    }
                    connection.incoming = None;
                }
            }
            Action::Close(id) => {
                if self
                    .connections
                    .get(&id)
                    .is_some_and(|connection| connection.ready.is_some())
                {
                    self.fail(
                        id,
                        io::ErrorKind::ConnectionRefused,
                        "forward closed before connecting",
                    );
                } else {
                    self.connections.remove(&id);
                }
            }
            Action::FailedEndPoints(ids) => {
                for id in ids {
                    let pending = self.connections.get(&id).is_some_and(|c| c.ready.is_some());
                    let kind = if pending {
                        io::ErrorKind::ConnectionRefused
                    } else {
                        io::ErrorKind::ConnectionReset
                    };
                    self.fail(id, kind, "forward endpoint failed");
                }
            }
            Action::Error(message) => return Err(io::Error::other(message)),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected server action",
                ));
            }
        }
        Ok(())
    }
}
