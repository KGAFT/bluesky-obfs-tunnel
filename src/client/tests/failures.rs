use super::*;

#[tokio::test]
async fn failed_opens_and_invalid_udp_operations_do_not_stop_other_forwards() {
    deadline(async {
        let (address, origin) = tcp_echo(1).await;
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let mut connection = client.open_tcp(address.to_string()).await.unwrap();
        assert!(matches!(client.open_tcp("invalid socket address").await, Err(error) if error.kind() == io::ErrorKind::ConnectionRefused));
        assert!(matches!(client.open_udp("invalid socket address").await, Err(error) if error.kind() == io::ErrorKind::ConnectionRefused));
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp = client.open_udp(socket.local_addr().unwrap().to_string()).await.unwrap();
        assert_eq!(udp.send(Bytes::from(vec![0; MAX_TUNNEL_CHUNK + 1])).await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert_eq!(udp.shutdown_write().await.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        connection.send(Bytes::from_static(b"unaffected")).await.unwrap();
        assert_eq!(read_bytes(&mut connection, 10).await, b"unaffected");
        drop((connection, udp));
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap(); server.await.unwrap().unwrap(); origin.await.unwrap();
    }).await;
}

#[tokio::test]
async fn oversized_udp_response_fails_only_its_connection() {
    deadline(async {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (address, origin) = tcp_echo(1).await;
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let mut udp = client
            .open_udp(socket.local_addr().unwrap().to_string())
            .await
            .unwrap();
        let mut tcp = client.open_tcp(address.to_string()).await.unwrap();
        udp.send(Bytes::from_static(b"request")).await.unwrap();
        let (_, source) = socket.recv_from(&mut [0; 32]).await.unwrap();
        socket
            .send_to(&vec![0; MAX_TUNNEL_CHUNK + 1], source)
            .await
            .unwrap();
        assert_eq!(
            udp.recv().await.unwrap_err().kind(),
            io::ErrorKind::ConnectionReset
        );
        tcp.send(Bytes::from_static(b"alive")).await.unwrap();
        assert_eq!(read_bytes(&mut tcp, 5).await, b"alive");
        drop((tcp, udp));
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
        origin.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn tunnel_failure_is_reported_to_active_connections() {
    deadline(async {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let mut connection = client
            .open_udp(socket.local_addr().unwrap().to_string())
            .await
            .unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        assert!(driver.await.unwrap().is_err());
        assert_eq!(
            connection.recv().await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(
            connection.send(Bytes::new()).await.unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    })
    .await;
}

#[tokio::test]
async fn full_receive_queue_reports_failure_after_buffered_data() {
    deadline(async {
        let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
        let limits = StreamLimits {
            max_pending_frames: 1,
            ..StreamLimits::default()
        };
        let (client, driver) = ForwardClient::from_framed(
            Framed::new(client_transport, LengthDelimitedCodec::new()),
            limits,
        );
        let mut peer = Framed::new(server_transport, LengthDelimitedCodec::new());
        let opener = client.clone();
        let open = tokio::spawn(async move { opener.open_udp("127.0.0.1:5353").await });
        let (Action::ConnectUdp((id, _)), _) =
            decode_frame(peer.next().await.unwrap().unwrap()).unwrap()
        else {
            panic!("expected UDP connect");
        };
        peer.send(encode_action(Action::Connected(id)).unwrap())
            .await
            .unwrap();
        let mut connection = open.await.unwrap().unwrap();
        for data in [b"first".as_slice(), b"overflow".as_slice()] {
            peer.send(crate::protocol::encode_frame(Action::UdpPacket(id), data).unwrap())
                .await
                .unwrap();
        }
        assert_eq!(
            decode_frame(peer.next().await.unwrap().unwrap()).unwrap(),
            (Action::Close(id), None)
        );
        assert_eq!(connection.recv().await.unwrap().unwrap(), b"first"[..]);
        assert_eq!(
            connection.recv().await.unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(connection.recv().await.unwrap().is_none());
        let peer_task = tokio::spawn(async move {
            assert!(peer.next().await.is_none());
        });
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        peer_task.await.unwrap();
    })
    .await;
}
