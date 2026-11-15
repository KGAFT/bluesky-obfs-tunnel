use super::*;

#[tokio::test]
async fn tcp_preserves_bytes_chunks_responses_and_supports_client_half_close() {
    deadline(async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let response: Vec<u8> = (0..MAX_TUNNEL_CHUNK * 3 + 77)
            .map(|i| (i % 251) as u8)
            .collect();
        let expected = response.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = origin.accept().await.unwrap();
            let mut request = Vec::new();
            socket.read_to_end(&mut request).await.unwrap();
            assert_eq!(request, b"helloworld");
            socket.write_all(&response).await.unwrap();
        });
        let (mut peer, task) = session(ForwardTo::Tcp(TcpConnection::new(
            address.ip(),
            address.port(),
        )));
        peer.send(Bytes::from_static(b"hello")).await.unwrap();
        peer.send(Bytes::from_static(b"world")).await.unwrap();
        peer.get_mut().shutdown().await.unwrap();
        let mut received = Vec::new();
        while let Some(frame) = peer.next().await {
            let frame = frame.unwrap();
            assert!(frame.len() <= MAX_TUNNEL_CHUNK);
            received.extend_from_slice(&frame);
        }
        assert_eq!(received, expected);
        task.await.unwrap().unwrap();
        server.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn tcp_target_eof_terminates_without_waiting_for_the_client() {
    deadline(async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let (mut peer, task) = session(ForwardTo::Tcp(TcpConnection::new(
            address.ip(),
            address.port(),
        )));
        let (socket, _) = origin.accept().await.unwrap();
        drop(socket);
        assert!(peer.next().await.is_none());
        task.await.unwrap().unwrap();
    })
    .await;
}
