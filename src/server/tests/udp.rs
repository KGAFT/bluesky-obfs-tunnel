use super::*;

#[tokio::test]
async fn udp_preserves_datagrams_including_empty_and_maximum_size() {
    deadline(async {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let (mut peer, task) = session(ForwardTo::Udp(UdpConnection::new(
            address.ip(),
            address.port(),
        )));
        let packets = [
            Bytes::from_static(b"first"),
            Bytes::new(),
            Bytes::from(vec![3; MAX_TUNNEL_CHUNK]),
            Bytes::from_static(b"last"),
        ];
        for packet in &packets {
            peer.send(packet.clone()).await.unwrap();
        }
        let mut buffer = vec![0; MAX_TUNNEL_CHUNK + 1];
        for packet in &packets {
            let (size, source) = socket.recv_from(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..size], &packet[..]);
            socket.send_to(&buffer[..size], source).await.unwrap();
        }
        for packet in &packets {
            assert_eq!(&peer.next().await.unwrap().unwrap()[..], &packet[..]);
        }
        drop(peer);
        task.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test]
async fn udp_sessions_use_separate_target_sockets() {
    deadline(async {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let make_target = || ForwardTo::Udp(UdpConnection::new(address.ip(), address.port()));
        let (mut peer1, task1) = session(make_target());
        let (mut peer2, task2) = session(make_target());
        peer1.send(Bytes::from_static(b"one")).await.unwrap();
        peer2.send(Bytes::from_static(b"two")).await.unwrap();
        let mut sources = Vec::new();
        let mut buffer = [0; 16];
        for _ in 0..2 {
            let (size, source) = socket.recv_from(&mut buffer).await.unwrap();
            sources.push(source);
            socket.send_to(&buffer[..size], source).await.unwrap();
        }
        assert_ne!(sources[0], sources[1]);
        assert_eq!(&peer1.next().await.unwrap().unwrap()[..], b"one");
        assert_eq!(&peer2.next().await.unwrap().unwrap()[..], b"two");
        drop((peer1, peer2));
        task1.await.unwrap().unwrap();
        task2.await.unwrap().unwrap();
    })
    .await;
}

#[tokio::test]
async fn oversized_udp_is_rejected_in_both_directions() {
    deadline(async {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let target = || ForwardTo::Udp(UdpConnection::new(address.ip(), address.port()));
        let (mut peer, task) = session(target());
        peer.send(Bytes::from(vec![0; MAX_TUNNEL_CHUNK + 1]))
            .await
            .unwrap();
        assert_eq!(
            task.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let (mut peer, task) = session(target());
        peer.send(Bytes::from_static(b"request")).await.unwrap();
        let (_, source) = socket.recv_from(&mut [0; 32]).await.unwrap();
        socket
            .send_to(&vec![0; MAX_TUNNEL_CHUNK + 1], source)
            .await
            .unwrap();
        assert_eq!(
            task.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(peer.next().await.is_none());
    })
    .await;
}
