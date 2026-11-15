use super::*;

// Independent reference layout from the original session protocol. Removed
// action slots remain here only to verify compatibility with existing peers.
mod legacy {
    use super::*;
    #[derive(Archive, Serialize, Deserialize)]
    pub enum SessionType {
        SessionHeader,
    }
    #[derive(Archive, Serialize, Deserialize)]
    pub enum Action {
        Connect((u64, String)),
        FailedEndPoints(Vec<u64>),
        BeginTun,
        TunIpAssign((Option<Vec<u8>>, Option<Vec<u8>>)),
        TunIpPacket,
        HttpPacket(u64),
        Error(String),
    }
    #[derive(Archive, Serialize, Deserialize)]
    pub struct Header {
        pub s_type: SessionType,
        pub action: Action,
        pub payload_len: u16,
    }
}

#[test]
fn http_frames_match_the_original_protocol_in_both_directions() {
    let cases = [
        (
            Action::Connect((42, "example.com:443".into())),
            legacy::Action::Connect((42, "example.com:443".into())),
            &b""[..],
        ),
        (
            Action::FailedEndPoints(vec![1, u64::MAX]),
            legacy::Action::FailedEndPoints(vec![1, u64::MAX]),
            &b""[..],
        ),
        (
            Action::HttpPacket(u64::MAX),
            legacy::Action::HttpPacket(u64::MAX),
            &b"payload"[..],
        ),
        (
            Action::Error("failed".into()),
            legacy::Action::Error("failed".into()),
            &b""[..],
        ),
    ];
    for (action, legacy_action, payload) in cases {
        let header = rkyv::to_bytes::<Error>(&legacy::Header {
            s_type: legacy::SessionType::SessionHeader,
            action: legacy_action,
            payload_len: payload.len() as u16,
        })
        .unwrap();
        let mut original = BytesMut::new();
        original.extend_from_slice(&(header.len() as u64).to_be_bytes());
        original.extend_from_slice(&header);
        original.extend_from_slice(payload);
        let encoded = encode_frame(action.clone(), payload).unwrap();
        assert_eq!(
            encoded[..],
            original[..],
            "wire layout changed for {action:?}"
        );
        let (decoded, data) = decode_frame(original).unwrap();
        assert_eq!(decoded, action);
        assert_eq!(data.as_deref().unwrap_or_default(), payload);
        // An original peer must also validate and read our serialized header.
        let end = HEADER_LEN_PREFIX + header.len();
        let mut aligned = AlignedVec::<16>::new();
        aligned.extend_from_slice(&encoded[HEADER_LEN_PREFIX..end]);
        rkyv::access::<legacy::ArchivedHeader, Error>(&aligned).unwrap();
    }
}

#[test]
fn malformed_frames_and_invalid_payloads_are_rejected() {
    assert!(encode_frame(Action::HttpPacket(1), &[]).is_none());
    assert!(encode_frame(Action::Connect((1, "host:80".into())), b"x").is_none());
    assert!(encode_frame(Action::HttpPacket(1), &vec![0; MAX_TUNNEL_CHUNK + 1]).is_none());
    let full = encode_frame(Action::HttpPacket(7), b"payload").unwrap();
    for n in 0..full.len() {
        assert!(decode_frame(BytesMut::from(&full[..n])).is_none());
    }
    let mut extra = BytesMut::from(&full[..]);
    extra.extend_from_slice(b"extra");
    assert!(decode_frame(extra).is_none());
    assert!(decode_frame(BytesMut::from(&u64::MAX.to_be_bytes()[..])).is_none());
    assert!(decode_frame(BytesMut::from(&[0xff; 32][..])).is_none());
}
