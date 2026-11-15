use super::*;

#[tokio::test]
async fn endpoint_connect_buffers_are_bounded_and_duplicate_ids_are_rejected() {
    let mut endpoints = OneToManyStreams::new(
        StreamLimits {
            max_pending_frames: 1,
            ..StreamLimits::default()
        },
        ConnectPolicy::default(),
    );
    assert!(endpoints.begin_connect(1, "127.0.0.1:1".into()));
    assert!(!endpoints.begin_connect(1, "127.0.0.1:2".into()));
    assert!(endpoints.send_to(1, Bytes::from_static(b"first")));
    assert!(!endpoints.send_to(1, Bytes::from_static(b"overflow")));
}

#[tokio::test]
async fn dropping_endpoint_manager_cancels_connects_and_established_sockets() {
    deadline(async {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = origin.local_addr().unwrap();
        let mut endpoints =
            OneToManyStreams::new(StreamLimits::default(), ConnectPolicy::default());
        assert!(endpoints.begin_connect(1, address.to_string()));
        let (mut socket, _) = origin.accept().await.unwrap();
        socket.write_all(b"ready").await.unwrap();
        assert!(matches!(
            endpoints.receive().await,
            EndpointEvent::Data(1, _)
        ));
        drop(endpoints);
        assert_eq!(socket.read(&mut [0; 1]).await.unwrap(), 0);
    })
    .await;
}
