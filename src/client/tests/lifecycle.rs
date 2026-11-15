use super::*;

#[tokio::test]
async fn cancelling_an_open_closes_the_pending_id_and_ignores_a_late_ack() {
    deadline(async {
        let (client_transport, server_transport) = tokio::io::duplex(64 * 1024);
        let (client, driver) = ForwardClient::from_framed(
            Framed::new(client_transport, LengthDelimitedCodec::new()),
            StreamLimits::default(),
        );
        let mut peer = Framed::new(server_transport, LengthDelimitedCodec::new());
        let opener = client.clone();
        let task = tokio::spawn(async move { opener.open_tcp("127.0.0.1:1").await });
        let (Action::Connect((id, _)), _) =
            decode_frame(peer.next().await.unwrap().unwrap()).unwrap()
        else {
            panic!("expected connect");
        };
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(
            decode_frame(peer.next().await.unwrap().unwrap()).unwrap(),
            (Action::Close(id), None)
        );
        peer.send(encode_action(Action::Connected(id)).unwrap())
            .await
            .unwrap();
        let peer_task = tokio::spawn(async move {
            assert!(peer.next().await.is_none());
        });
        client.shutdown().await.unwrap();
        driver.await.unwrap().unwrap();
        peer_task.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn dropping_all_handles_stops_driver_and_closes_targets() {
    deadline(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, driver, server) = tunnel(ConnectPolicy::default());
        let connection = client
            .open_tcp(listener.local_addr().unwrap().to_string())
            .await
            .unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        drop(client);
        drop(connection);
        driver.await.unwrap().unwrap();
        server.await.unwrap().unwrap();
        assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
    })
    .await;
}
