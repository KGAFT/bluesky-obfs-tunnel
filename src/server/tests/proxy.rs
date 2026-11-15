use super::*;

#[tokio::test]
async fn http_proxy_multiplexes_immediate_data_and_reports_eof_after_responses() {
    deadline(async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            for _ in 0..2 {
                let (mut socket, _) = origin.accept().await.unwrap();
                connections.spawn(async move {
                    let mut request = [0; 10];
                    socket.read_exact(&mut request).await.unwrap();
                    assert_eq!(&request, b"helloworld");
                    socket.write_all(response).await.unwrap();
                });
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        let (mut peer, task) = session(ForwardTo::HttpProxy(ConnectPolicy::default()));
        for id in [7, u64::MAX] {
            send_action(&mut peer, Action::Connect((id, address.to_string()))).await;
            for data in [b"hello", b"world"] {
                peer.send(encode_frame(Action::HttpPacket(id), data).unwrap())
                    .await
                    .unwrap();
            }
        }
        let mut responses = std::collections::HashMap::<u64, Vec<u8>>::new();
        let mut closed = Vec::new();
        while closed.len() < 2 {
            match receive_action(&mut peer).await {
                (Action::HttpPacket(id), Some(data)) => {
                    assert!(!closed.contains(&id));
                    responses.entry(id).or_default().extend_from_slice(&data);
                }
                (Action::FailedEndPoints(ids), None) => closed.extend(ids),
                other => panic!("unexpected proxy frame: {other:?}"),
            }
        }
        assert_eq!(responses[&7], response);
        assert_eq!(responses[&u64::MAX], response);
        drop(peer);
        task.await.unwrap().unwrap();
        server.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn http_failed_connect_unknown_id_and_policy_rejection_are_reported() {
    deadline(async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let (mut peer, task) = session(ForwardTo::HttpProxy(ConnectPolicy::new(false, &[], &[])));
        send_action(&mut peer, Action::Connect((1, address.to_string()))).await;
        assert_eq!(
            receive_action(&mut peer).await,
            (Action::FailedEndPoints(vec![1]), None)
        );
        peer.send(encode_frame(Action::HttpPacket(2), b"missing").unwrap())
            .await
            .unwrap();
        assert_eq!(
            receive_action(&mut peer).await,
            (Action::FailedEndPoints(vec![2]), None)
        );
        drop(peer);
        task.await.unwrap().unwrap();
        let (mut peer, task) = session(ForwardTo::HttpProxy(ConnectPolicy::default()));
        send_action(
            &mut peer,
            Action::Connect((3, "invalid socket address".into())),
        )
        .await;
        assert_eq!(
            receive_action(&mut peer).await,
            (Action::FailedEndPoints(vec![3]), None)
        );
        drop(peer);
        task.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test]
async fn disconnecting_proxy_client_closes_its_endpoints() {
    deadline(async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let (mut peer, task) = session(ForwardTo::HttpProxy(ConnectPolicy::default()));
        send_action(&mut peer, Action::Connect((1, address.to_string()))).await;
        let (mut socket, _) = origin.accept().await.unwrap();
        drop(peer);
        task.await.unwrap().unwrap();
        assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
    })
    .await;
}
