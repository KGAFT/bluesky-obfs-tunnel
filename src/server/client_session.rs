use crate::server::{
    StreamLimits,
    endpoints::{EndpointEvent, OneToManyStreams},
    fwd_server::ForwardTo,
    protocol::{
        Action, ForwardProtocol, MAX_TUNNEL_CHUNK, decode_frame, encode_action, encode_frame,
    },
    tcp_connection::TcpConnection,
    udp_connection::UdpConnection,
};
use bsobfs::the_fourth_utils::futures_util::{SinkExt, StreamExt, stream::SplitStream};
use std::{io, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};
use tokio_util::{
    bytes::{Bytes, BytesMut},
    codec::{Decoder, Encoder, Framed},
};

pub struct ClientSession<C, T = TcpStream> {
    stream: Framed<T, C>,
    target: Arc<ForwardTo>,
    limits: StreamLimits,
}

impl<C, T> ClientSession<C, T>
where
    C: Decoder<Item = BytesMut, Error = io::Error> + Encoder<Bytes, Error = io::Error> + Unpin,
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(stream: Framed<T, C>, target: Arc<ForwardTo>, limits: StreamLimits) -> Self {
        Self {
            stream,
            target,
            limits,
        }
    }

    pub async fn run(self) -> io::Result<()> {
        let Self {
            stream,
            target,
            limits,
        } = self;
        let (mut sink, mut reader) = stream.split();
        let (tx, mut rx) = mpsc::channel::<Bytes>(limits.sink_queue_depth.max(1));
        let forward = async {
            match target.as_ref() {
                ForwardTo::Tcp(target) => Self::forward_tcp(target, &mut reader, &tx).await?,
                ForwardTo::Udp(target) => Self::forward_udp(target, &mut reader, &tx).await?,
                ForwardTo::HttpProxy(policy) => {
                    let endpoints = OneToManyStreams::new(limits, policy.clone());
                    Self::proxy(endpoints, &mut reader, &tx, false).await?;
                }
                ForwardTo::Multiplexed(policy) => {
                    let endpoints = OneToManyStreams::new(limits, policy.clone());
                    Self::proxy(endpoints, &mut reader, &tx, true).await?;
                }
            }
            drop(tx);
            Ok::<(), io::Error>(())
        };
        let write = async {
            while let Some(frame) = rx.recv().await {
                sink.send(frame).await?;
            }
            sink.close().await
        };
        // Sink errors cancel forwarding and all endpoint tasks. Clean EOF drains
        // queued responses before closing the sink, without a detached writer.
        tokio::try_join!(forward, write)?;
        Ok(())
    }

    async fn forward_tcp(
        target: &TcpConnection,
        reader: &mut SplitStream<Framed<T, C>>,
        tx: &mpsc::Sender<Bytes>,
    ) -> io::Result<()> {
        let target = target.instantiate().await?;
        let (mut target_read, mut target_write) = target.into_split();
        let outgoing = async {
            while let Some(data) = reader.next().await {
                target_write.write_all(&data?).await?;
            }
            target_write.shutdown().await
        };
        let incoming = async {
            let mut buffer = vec![0; MAX_TUNNEL_CHUNK];
            loop {
                let size = target_read.read(&mut buffer).await?;
                if size == 0 {
                    return Ok::<(), io::Error>(());
                }
                send(tx, Bytes::copy_from_slice(&buffer[..size])).await?;
            }
        };
        tokio::pin!(outgoing, incoming);
        tokio::select! {
            result = &mut outgoing => { result?; incoming.await }
            result = &mut incoming => result,
        }
    }

    async fn forward_udp(
        target: &UdpConnection,
        reader: &mut SplitStream<Framed<T, C>>,
        tx: &mpsc::Sender<Bytes>,
    ) -> io::Result<()> {
        let socket = target.instantiate().await?;
        // Receive the whole datagram so oversize packets cannot be truncated.
        let mut buffer = vec![0; u16::MAX as usize + 1];
        loop {
            tokio::select! {
                frame = reader.next() => {
                    let Some(frame) = frame else { return Ok(()) };
                    let frame = frame?;
                    check_datagram_size(frame.len())?;
                    let written = socket.send(&frame).await?;
                    if written != frame.len() {
                        return Err(io::Error::new(io::ErrorKind::WriteZero, "partial UDP send"));
                    }
                }
                size = socket.recv(&mut buffer) => {
                    let size = size?;
                    check_datagram_size(size)?;
                    send(tx, Bytes::copy_from_slice(&buffer[..size])).await?;
                }
            }
        }
    }

    async fn proxy(
        mut endpoints: OneToManyStreams,
        reader: &mut SplitStream<Framed<T, C>>,
        tx: &mpsc::Sender<Bytes>,
        multiplexed: bool,
    ) -> io::Result<()> {
        loop {
            tokio::select! {
                frame = reader.next() => {
                    let Some(frame) = frame else { return Ok(()) };
                    let (action, payload) = decode_frame(frame?).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP proxy frame")
                    })?;
                    match action {
                        Action::Connect((id, destination)) => {
                            let started = if multiplexed {
                                endpoints.begin_forward(id, destination, ForwardProtocol::Tcp)
                            } else {
                                endpoints.begin_connect(id, destination)
                            };
                            if !started {
                                report_failed(tx, id).await?;
                            }
                        }
                        Action::HttpPacket(id) => {
                            if endpoints.protocol(id) != Some(ForwardProtocol::Tcp)
                                || !endpoints.send_to(id, payload.expect("validated payload")) {
                                endpoints.close(id);
                                report_failed(tx, id).await?;
                            }
                        }
                        Action::ConnectUdp((id, destination)) if multiplexed => {
                            if !endpoints.begin_forward(id, destination, ForwardProtocol::Udp) {
                                report_failed(tx, id).await?;
                            }
                        }
                        Action::UdpPacket(id) if multiplexed => {
                            if endpoints.protocol(id) != Some(ForwardProtocol::Udp)
                                || !endpoints.send_to(id, payload.expect("validated UDP payload")) {
                                endpoints.close(id);
                                report_failed(tx, id).await?;
                            }
                        }
                        Action::ShutdownWrite(id) if multiplexed => {
                            if !endpoints.shutdown_write(id) {
                                endpoints.close(id);
                                report_failed(tx, id).await?;
                            }
                        }
                        Action::Close(id) if multiplexed => {
                            endpoints.close(id);
                            send_action(tx, Action::Close(id)).await?;
                        }
                        Action::FailedEndPoints(_) | Action::Error(_) => {}
                        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported client action")),
                    }
                }
                event = endpoints.receive() => {
                    match event {
                        EndpointEvent::Connected(id) => send_action(tx, Action::Connected(id)).await?,
                        EndpointEvent::Data(id, data) => {
                            if endpoints.protocol(id) == Some(ForwardProtocol::Udp) {
                                let frame = encode_frame(Action::UdpPacket(id), &data).ok_or_else(|| {
                                    io::Error::new(io::ErrorKind::InvalidData, "cannot encode UDP data")
                                })?;
                                send(tx, frame).await?;
                            } else {
                                send_to_client(tx, data, id).await?;
                            }
                        }
                        EndpointEvent::ReadClosed(id) => send_action(tx, Action::ShutdownWrite(id)).await?,
                        EndpointEvent::Closed(id) if multiplexed => send_action(tx, Action::Close(id)).await?,
                        EndpointEvent::Closed(id) | EndpointEvent::Failed(id) => report_failed(tx, id).await?,
                    }
                }
            }
        }
    }
}

async fn send(tx: &mpsc::Sender<Bytes>, data: Bytes) -> io::Result<()> {
    tx.send(data)
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
}

async fn report_failed(tx: &mpsc::Sender<Bytes>, id: u64) -> io::Result<()> {
    send_action(tx, Action::FailedEndPoints(vec![id])).await
}

async fn send_action(tx: &mpsc::Sender<Bytes>, action: Action) -> io::Result<()> {
    send(
        tx,
        encode_action(action).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "cannot encode control frame")
        })?,
    )
    .await
}

async fn send_to_client(tx: &mpsc::Sender<Bytes>, mut data: Bytes, id: u64) -> io::Result<()> {
    while !data.is_empty() {
        let chunk = data.split_to(data.len().min(MAX_TUNNEL_CHUNK));
        let frame = encode_frame(Action::HttpPacket(id), &chunk).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "cannot encode HTTP proxy data")
        })?;
        send(tx, frame).await?;
    }
    Ok(())
}

fn check_datagram_size(size: usize) -> io::Result<()> {
    if size > MAX_TUNNEL_CHUNK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("UDP datagram of {size} bytes exceeds frame limit {MAX_TUNNEL_CHUNK}"),
        ));
    }
    Ok(())
}
