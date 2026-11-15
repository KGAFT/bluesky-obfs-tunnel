use super::*;

#[tokio::test]
async fn multiple_tcp_and_udp_connections_share_one_tunnel_without_crosstalk() {
    deadline(async {
        let (tcp1, origin1) = tcp_echo(1).await;
        let (tcp2, origin2) = tcp_echo(1).await;
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let udp_origin = tokio::spawn(async move {
            let mut sources = Vec::new();
            let mut buffer = vec![0; MAX_TUNNEL_CHUNK + 1];
            for _ in 0..4 {
                let (size, source) = udp.recv_from(&mut buffer).await.unwrap();
                if !sources.contains(&source) {
                    sources.push(source);
                }
                udp.send_to(&buffer[..size], source).await.unwrap();
            }
            assert_eq!(sources.len(), 2, "UDP endpoints shared a source socket");
        });
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let (tcp_a, tcp_b, udp_a, udp_b) = tokio::join!(
            client.open_tcp(tcp1.to_string()),
            client.open_tcp(tcp2.to_string()),
            client.open_udp(udp_address.to_string()),
            client.open_udp(udp_address.to_string()),
        );
        let (mut tcp_a, mut tcp_b, mut udp_a, mut udp_b) = (
            tcp_a.unwrap(),
            tcp_b.unwrap(),
            udp_a.unwrap(),
            udp_b.unwrap(),
        );
        let mut ids = vec![tcp_a.id(), tcp_b.id(), udp_a.id(), udp_b.id()];
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert_eq!(tcp_a.protocol(), ForwardProtocol::Tcp);
        assert_eq!(udp_a.protocol(), ForwardProtocol::Udp);
        let tcp_data = Bytes::from(vec![7; MAX_TUNNEL_CHUNK * 3 + 77]);
        let (a, b, c, d) = tokio::join!(
            tcp_a.send(tcp_data.clone()),
            tcp_b.send(Bytes::from_static(b"second TCP")),
            udp_a.send(Bytes::from_static(b"first UDP")),
            udp_b.send(Bytes::from_static(b"second UDP")),
        );
        a.unwrap();
        b.unwrap();
        c.unwrap();
        d.unwrap();
        udp_a.send(Bytes::new()).await.unwrap();
        let full_datagram = Bytes::from(vec![5; MAX_TUNNEL_CHUNK]);
        udp_b.send(full_datagram.clone()).await.unwrap();
        assert_eq!(read_bytes(&mut tcp_a, tcp_data.len()).await, tcp_data);
        assert_eq!(read_bytes(&mut tcp_b, 10).await, b"second TCP");
        assert_eq!(udp_a.recv().await.unwrap().unwrap(), b"first UDP"[..]);
        assert_eq!(udp_a.recv().await.unwrap(), Some(Bytes::new()));
        assert_eq!(udp_b.recv().await.unwrap().unwrap(), b"second UDP"[..]);
        assert_eq!(udp_b.recv().await.unwrap(), Some(full_datagram));
        drop((tcp_a, tcp_b, udp_a, udp_b));
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
        origin1.await.unwrap();
        origin2.await.unwrap();
        udp_origin.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn tcp_half_close_keeps_its_response_and_other_connections_alive() {
    deadline(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            socket.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"request");
            socket.write_all(b"response after EOF").await.unwrap();
        });
        let (echo, echo_task) = tcp_echo(1).await;
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let mut connection = client.open_tcp(address.to_string()).await.unwrap();
        let mut other = client.open_tcp(echo.to_string()).await.unwrap();
        connection
            .send(Bytes::from_static(b"request"))
            .await
            .unwrap();
        connection.shutdown_write().await.unwrap();
        assert_eq!(
            connection
                .send(Bytes::from_static(b"late"))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(read_bytes(&mut connection, 18).await, b"response after EOF");
        assert!(connection.recv().await.unwrap().is_none());
        other
            .send(Bytes::from_static(b"still connected"))
            .await
            .unwrap();
        assert_eq!(read_bytes(&mut other, 15).await, b"still connected");
        drop((connection, other));
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
        origin.await.unwrap();
        echo_task.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn remote_tcp_read_eof_still_allows_client_writes() {
    deadline(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let origin = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket.shutdown().await.unwrap();
            let mut request = Vec::new();
            socket.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"after remote half-close");
        });
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let mut connection = client.open_tcp(address.to_string()).await.unwrap();
        assert!(connection.recv().await.unwrap().is_none());
        connection
            .send(Bytes::from_static(b"after remote half-close"))
            .await
            .unwrap();
        connection.shutdown_write().await.unwrap();
        origin.await.unwrap();
        drop(connection);
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test]
async fn dropping_one_forward_closes_only_its_target_and_split_halves_work() {
    deadline(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (echo, echo_task) = tcp_echo(1).await;
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let connection = client.open_tcp(address.to_string()).await.unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        let other = client.open_tcp(echo.to_string()).await.unwrap();
        let (sender, mut receiver) = other.split();
        drop(connection);
        assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
        sender
            .send(Bytes::from_static(b"independent"))
            .await
            .unwrap();
        assert_eq!(receiver.recv().await.unwrap().unwrap(), b"independent"[..]);
        sender.close().await.unwrap();
        assert!(receiver.recv().await.unwrap().is_none());
        assert_eq!(
            sender
                .send(Bytes::from_static(b"closed"))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::BrokenPipe
        );
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
        echo_task.await.unwrap();
    })
    .await;
}
