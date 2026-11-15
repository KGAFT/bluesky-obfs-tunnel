use super::*;
use crate::StreamLimits;
use tokio::{
    io::{AsyncReadExt, DuplexStream},
    sync::mpsc,
    time::{advance, sleep},
};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

fn session() -> (ClientSession, DuplexStream) {
    let (client, server) = tokio::io::duplex(4096);
    (
        ForwardClient::from_framed(
            Framed::new(client, LengthDelimitedCodec::new()),
            StreamLimits::default(),
        ),
        server,
    )
}

fn start(
    config: ReconnectConfig,
) -> (
    JoinHandle<io::Result<()>>,
    mpsc::UnboundedReceiver<DuplexStream>,
    mpsc::UnboundedReceiver<ForwardClient>,
) {
    let (peers, received_peers) = mpsc::unbounded_channel();
    let (clients, received_clients) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        supervise(
            &config,
            Duration::from_secs(5),
            || {
                let (session, peer) = session();
                peers.send(peer).unwrap();
                async { Ok(session) }
            },
            |client| {
                clients.send(client).unwrap();
                pending::<io::Result<()>>()
            },
        )
        .await
    });
    (task, received_peers, received_clients)
}

fn fixed() -> ReconnectConfig {
    ReconnectConfig {
        session_min_secs: 10,
        session_max_secs: 10,
        retry_delay_secs: 2,
        enabled: true,
    }
}

#[tokio::test(start_paused = true)]
async fn rotation_closes_old_transport_and_cancelling_supervisor_closes_new_one() {
    let (task, mut peers, mut clients) = start(fixed());
    let mut first_peer = peers.recv().await.unwrap();
    let first = clients.recv().await.unwrap();
    advance(Duration::from_secs(9)).await;
    assert!(peers.try_recv().is_err());
    advance(Duration::from_secs(1)).await;
    let mut second_peer = peers.recv().await.unwrap();
    let _second = clients.recv().await.unwrap();
    assert_eq!(first_peer.read(&mut [0; 1]).await.unwrap(), 0);
    assert!(first.open_tcp("localhost:80").await.is_err());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(second_peer.read(&mut [0; 1]).await.unwrap(), 0);
}

#[tokio::test(start_paused = true)]
async fn disabled_rotation_keeps_session_and_failures_retry_after_configured_delay() {
    let (task, mut peers, mut clients) = start(ReconnectConfig {
        enabled: false,
        ..fixed()
    });
    let first_peer = peers.recv().await.unwrap();
    let _first = clients.recv().await.unwrap();
    advance(Duration::from_secs(24 * 60 * 60)).await;
    assert!(peers.try_recv().is_err());
    drop(first_peer);
    tokio::task::yield_now().await;
    advance(Duration::from_secs(1)).await;
    assert!(peers.try_recv().is_err());
    advance(Duration::from_secs(1)).await;
    let _new_peer = peers.recv().await.unwrap();
    assert!(clients.recv().await.is_some());
    task.abort();
    let _ = task.await;
}

#[tokio::test(start_paused = true)]
async fn initial_connect_errors_and_timeouts_retry_and_service_errors_are_fatal() {
    let (attempts, mut received) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let mut attempt = 0;
        let (held_peers, _peers) = mpsc::unbounded_channel();
        supervise(
            &fixed(),
            Duration::from_secs(3),
            || {
                attempt += 1;
                attempts
                    .send((attempt, tokio::time::Instant::now()))
                    .unwrap();
                let current = attempt;
                let held_peers = held_peers.clone();
                async move {
                    if current == 1 {
                        return Err(io::Error::other("offline"));
                    }
                    if current == 2 {
                        sleep(Duration::from_secs(10)).await;
                    }
                    let (session, peer) = session();
                    held_peers.send(peer).unwrap();
                    Ok(session)
                }
            },
            |_| async { Err(io::Error::other("listener failed")) },
        )
        .await
    });
    let (_, first_time) = received.recv().await.unwrap();
    let (_, second_time) = received.recv().await.unwrap();
    let (_, third_time) = received.recv().await.unwrap();
    assert_eq!(second_time - first_time, Duration::from_secs(2));
    assert_eq!(third_time - second_time, Duration::from_secs(5));
    assert_eq!(
        task.await.unwrap().unwrap_err().to_string(),
        "listener failed"
    );
}
