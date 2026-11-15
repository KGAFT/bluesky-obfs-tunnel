//! Connects destination sockets and relays TCP streams or UDP datagrams.
use super::{EndpointEvent, Input};
use crate::{protocol::MAX_TUNNEL_CHUNK, server::connect_policy::ConnectPolicy};
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    sync::mpsc,
};
use tokio_util::bytes::Bytes;

pub(super) struct RelayContext {
    pub(super) id: u64,
    pub(super) generation: u64,
    pub(super) events: mpsc::Sender<(u64, EndpointEvent)>,
    pub(super) multiplexed: bool,
}

impl RelayContext {
    pub(super) async fn emit(&self, event: EndpointEvent) -> io::Result<()> {
        self.events
            .send((self.generation, event))
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
    }

    async fn connected(&self) -> io::Result<()> {
        if self.multiplexed {
            self.emit(EndpointEvent::Connected(self.id)).await?;
        }
        Ok(())
    }

    pub(super) async fn tcp(
        &self,
        destination: String,
        policy: ConnectPolicy,
        mut incoming: mpsc::Receiver<Input>,
    ) -> io::Result<()> {
        let connect = async {
            let addresses = policy.resolve(&destination).await?;
            TcpStream::connect(&addresses[..]).await
        };
        let stream = tokio::time::timeout(Duration::from_secs(10), connect).await??;
        stream.set_nodelay(true)?;
        self.connected().await?;
        let (mut reader, mut writer) = stream.into_split();
        let write = async {
            while let Some(input) = incoming.recv().await {
                match input {
                    Input::Data(data) => writer.write_all(&data).await?,
                    Input::ShutdownWrite => break,
                }
            }
            writer.shutdown().await
        };
        let read = async {
            let mut buffer = vec![0; MAX_TUNNEL_CHUNK];
            loop {
                let size = reader.read(&mut buffer).await?;
                if size == 0 {
                    if self.multiplexed {
                        self.emit(EndpointEvent::ReadClosed(self.id)).await?;
                    }
                    return Ok::<(), io::Error>(());
                }
                self.emit(EndpointEvent::Data(
                    self.id,
                    Bytes::copy_from_slice(&buffer[..size]),
                ))
                .await?;
            }
        };
        tokio::pin!(write, read);
        if self.multiplexed {
            tokio::try_join!(write, read)?;
            Ok(())
        } else {
            tokio::select! {
                result = &mut write => { result?; read.await }
                result = &mut read => result,
            }
        }
    }

    pub(super) async fn udp(
        &self,
        destination: String,
        policy: ConnectPolicy,
        mut incoming: mpsc::Receiver<Input>,
    ) -> io::Result<()> {
        let connect = async {
            let addresses = policy.resolve(&destination).await?;
            let mut last_error = io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "no UDP destination addresses",
            );
            for address in addresses {
                let bind = if address.is_ipv4() {
                    "0.0.0.0:0"
                } else {
                    "[::]:0"
                };
                let result = async {
                    let socket = UdpSocket::bind(bind).await?;
                    socket.connect(address).await?;
                    Ok::<_, io::Error>(socket)
                }
                .await;
                match result {
                    Ok(socket) => return Ok(socket),
                    Err(error) => last_error = error,
                }
            }
            Err(last_error)
        };
        let socket = tokio::time::timeout(Duration::from_secs(10), connect).await??;
        self.connected().await?;
        let mut buffer = vec![0; u16::MAX as usize + 1];
        loop {
            tokio::select! {
                input = incoming.recv() => {
                    let Some(Input::Data(data)) = input else { return Ok(()); };
                    if data.len() > MAX_TUNNEL_CHUNK {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "UDP datagram too large"));
                    }
                    if socket.send(&data).await? != data.len() {
                        return Err(io::Error::from(io::ErrorKind::WriteZero));
                    }
                }
                size = socket.recv(&mut buffer) => {
                    let size = size?;
                    if size > MAX_TUNNEL_CHUNK {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "UDP datagram too large"));
                    }
                    self.emit(EndpointEvent::Data(self.id, Bytes::copy_from_slice(&buffer[..size]))).await?;
                }
            }
        }
    }
}
