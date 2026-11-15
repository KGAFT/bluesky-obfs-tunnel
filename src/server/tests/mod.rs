use super::{
    StreamLimits,
    client_session::ClientSession,
    connect_policy::ConnectPolicy,
    endpoints::{EndpointEvent, OneToManyStreams},
    fwd_server::ForwardTo,
    protocol::{Action, MAX_TUNNEL_CHUNK, decode_frame, encode_action, encode_frame},
    tcp_connection::TcpConnection,
    udp_connection::UdpConnection,
};
use bsobfs::the_fourth_utils::futures_util::{SinkExt, StreamExt};
use std::{future::Future, io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    net::{TcpListener, UdpSocket},
    task::JoinHandle,
};
use tokio_util::{
    bytes::Bytes,
    codec::{Framed, LengthDelimitedCodec},
};

type Peer = Framed<DuplexStream, LengthDelimitedCodec>;

async fn deadline<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test timed out")
}

fn session(target: ForwardTo) -> (Peer, JoinHandle<io::Result<()>>) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let session = ClientSession::new(
        Framed::new(server, LengthDelimitedCodec::new()),
        Arc::new(target),
        StreamLimits::default(),
    );
    (
        Framed::new(client, LengthDelimitedCodec::new()),
        tokio::spawn(session.run()),
    )
}

async fn send_action(peer: &mut Peer, action: Action) {
    peer.send(encode_action(action).unwrap()).await.unwrap();
}

async fn receive_action(peer: &mut Peer) -> (Action, Option<Bytes>) {
    decode_frame(peer.next().await.unwrap().unwrap()).unwrap()
}

mod endpoints;
mod proxy;
mod tcp;
mod udp;
