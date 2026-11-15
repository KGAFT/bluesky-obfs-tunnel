use super::{
    options::Options,
    transfer::{multiplexed, raw_tcp},
};
use bluesky_obfs_tunnel::server::{
    client_session::ClientSession, connect_policy::ConnectPolicy, fwd_server::ForwardTo,
};
use bluesky_obfs_tunnel::{StreamLimits, client::ForwardClient, protocol::MAX_TUNNEL_CHUNK};
use bsobfs::the_fourth_utils::futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, UdpSocket},
};
use tokio_util::codec::LengthDelimitedCodec;
use tokio_util::{bytes::Bytes, codec::Framed};

#[test]
fn rejects_invalid_invocations() {
    for args in [
        vec!["--tcp", "localhost:80", "--udp", "localhost:53"],
        vec!["--message", "test", "--file", "payload.bin"],
        vec!["--timeout", "0"],
        vec!["--timeout", "NaN"],
        vec!["--setup-port", "65536"],
        vec!["--server"],
        vec!["--server", "--tcp", "localhost:80"],
        vec!["--unknown"],
        vec!["--config"],
        vec!["--config="],
        vec!["--config", ""],
        vec!["--proxy", "--tcp", "localhost:80"],
        vec!["--http-proxy"],
        vec!["--http-proxy", "127.0.0.1:8080", "--tcp", "localhost:80"],
        vec!["--http-proxy", "127.0.0.1:8080", "--udp", "localhost:53"],
        vec!["--http-proxy", "127.0.0.1:8080", "--message", "test"],
        vec!["--http-proxy", "127.0.0.1:8080", "--file", "payload.bin"],
    ] {
        assert!(
            Options::parse(args.iter().map(|arg| arg.to_string())).is_err(),
            "{args:?}"
        );
    }
}

#[test]
fn config_arguments_preserve_file_values_and_cli_overrides_win() {
    use bluesky_obfs_tunnel::config::ClientSettings;
    for args in [
        vec!["--config", "custom.json", "--proxy"],
        vec!["--config=custom.json", "--proxy"],
    ] {
        let options = Options::parse(args.into_iter().map(String::from))
            .unwrap()
            .unwrap();
        assert_eq!(options.config, "custom.json");
        assert!(options.is_proxy());
        let mut settings = ClientSettings {
            server_address: "from-file:443".into(),
            ..ClientSettings::default()
        };
        options.apply(&mut settings).unwrap();
        assert_eq!(settings.server_address, "from-file:443");
    }
    let options = Options::parse(
        [
            "--server",
            "from-cli:8443",
            "--http-proxy",
            "127.0.0.1:9912",
            "--timeout",
            "9",
            "--setup-port",
            "9001",
        ]
        .map(String::from),
    )
    .unwrap()
    .unwrap();
    let mut settings = ClientSettings::default();
    options.apply(&mut settings).unwrap();
    assert_eq!(settings.server_address, "from-cli:8443");
    assert_eq!(settings.local_proxy_bind, "127.0.0.1:9912");
    assert_eq!(settings.proxy_setup_timeout_secs, 9);
    assert_eq!(settings.obfuscation.setup_proxy_port, 9001);
}

#[tokio::test]
async fn raw_tcp_chunks_payload_and_reads_reply_after_half_close() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (client, server) = tokio::io::duplex(1024);
        let payload = Bytes::from(vec![42; MAX_TUNNEL_CHUNK * 4 + 3]);
        let expected = payload.clone();
        let server = tokio::spawn(async move {
            let mut stream = Framed::new(server, LengthDelimitedCodec::new());
            let mut received = Vec::new();
            while let Some(frame) = stream.next().await {
                let frame = frame.unwrap();
                assert!(frame.len() <= MAX_TUNNEL_CHUNK);
                received.extend_from_slice(&frame);
                stream.send(frame.freeze()).await.unwrap();
            }
            assert_eq!(received, expected);
            stream.send(Bytes::from_static(b"after EOF")).await.unwrap();
            SinkExt::<Bytes>::close(&mut stream).await.unwrap();
        });
        let mut output = Vec::new();
        raw_tcp(
            Framed::new(client, LengthDelimitedCodec::new()),
            payload.clone(),
            &mut output,
        )
        .await
        .unwrap();
        assert_eq!(&output[..payload.len()], &payload[..]);
        assert_eq!(&output[payload.len()..], b"after EOF");
        server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn multiplexed_tcp_and_udp_round_trips() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_address = tcp.local_addr().unwrap().to_string();
        let origin = tokio::spawn(async move {
            let (mut socket, _) = tcp.accept().await.unwrap();
            let mut request = Vec::new();
            socket.read_to_end(&mut request).await.unwrap();
            socket.write_all(&request).await.unwrap();
        });
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap().to_string();
        let udp_origin = tokio::spawn(async move {
            let mut buffer = vec![0; MAX_TUNNEL_CHUNK + 1];
            for _ in 0..2 {
                let (size, peer) = udp.recv_from(&mut buffer).await.unwrap();
                udp.send_to(&buffer[..size], peer).await.unwrap();
            }
        });
        let (client, server) = tokio::io::duplex(1024);
        let server = tokio::spawn(
            ClientSession::new(
                Framed::new(server, LengthDelimitedCodec::new()),
                Arc::new(ForwardTo::Multiplexed(ConnectPolicy::default())),
                StreamLimits::default(),
            )
            .run(),
        );
        let (client, driver) = ForwardClient::from_framed(
            Framed::new(client, LengthDelimitedCodec::new()),
            StreamLimits::default(),
        );
        let tcp_payload = Bytes::from(vec![7; MAX_TUNNEL_CHUNK * 3 + 17]);
        let udp_payload = Bytes::from(vec![9; MAX_TUNNEL_CHUNK]);
        let mut tcp_output = Vec::new();
        let mut udp_output = Vec::new();
        tokio::try_join!(
            multiplexed(
                client.open_tcp(tcp_address).await.unwrap(),
                tcp_payload.clone(),
                &mut tcp_output
            ),
            multiplexed(
                client.open_udp(udp_address.clone()).await.unwrap(),
                udp_payload.clone(),
                &mut udp_output
            ),
        )
        .unwrap();
        assert_eq!(tcp_output, tcp_payload);
        assert_eq!(udp_output, udp_payload);
        let mut empty_output = Vec::new();
        multiplexed(
            client.open_udp(udp_address).await.unwrap(),
            Bytes::new(),
            &mut empty_output,
        )
        .await
        .unwrap();
        assert!(empty_output.is_empty());
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
        origin.await.unwrap();
        udp_origin.await.unwrap();
    })
    .await
    .unwrap();
}
