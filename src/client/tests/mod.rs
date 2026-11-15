use super::*;
use crate::{
    StreamLimits,
    protocol::{Action, MAX_TUNNEL_CHUNK, decode_frame, encode_action},
    server::{client_session::ClientSession, connect_policy::ConnectPolicy, fwd_server::ForwardTo},
};
use bsobfs::the_fourth_utils::futures_util::{SinkExt, StreamExt};
use std::{future::Future, io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UdpSocket},
    task::{JoinHandle, JoinSet},
};
use tokio_util::{
    bytes::Bytes,
    codec::{Framed, LengthDelimitedCodec},
};

mod failures;
mod forwarding;
mod lifecycle;

async fn deadline<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test timed out")
}

fn tunnel(
    policy: ConnectPolicy,
) -> (
    ForwardClient,
    JoinHandle<io::Result<()>>,
    JoinHandle<io::Result<()>>,
) {
    let (client, server) = tokio::io::duplex(128 * 1024);
    let server = ClientSession::new(
        Framed::new(server, LengthDelimitedCodec::new()),
        Arc::new(ForwardTo::Multiplexed(policy)),
        StreamLimits::default(),
    );
    let server = tokio::spawn(server.run());
    let (client, driver) = ForwardClient::from_framed(
        Framed::new(client, LengthDelimitedCodec::new()),
        StreamLimits::default(),
    );
    (client, driver, server)
}

async fn tcp_echo(count: usize) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        for _ in 0..count {
            let (mut socket, _) = listener.accept().await.unwrap();
            tasks.spawn(async move {
                let mut buffer = vec![0; MAX_TUNNEL_CHUNK];
                loop {
                    let size = socket.read(&mut buffer).await.unwrap();
                    if size == 0 {
                        return;
                    }
                    socket.write_all(&buffer[..size]).await.unwrap();
                }
            });
        }
        while let Some(task) = tasks.join_next().await {
            task.unwrap();
        }
    });
    (address, task)
}

async fn read_bytes(connection: &mut ForwardConnection, length: usize) -> Vec<u8> {
    let mut received = Vec::new();
    while received.len() < length {
        received.extend_from_slice(&connection.recv().await.unwrap().unwrap());
    }
    received
}
