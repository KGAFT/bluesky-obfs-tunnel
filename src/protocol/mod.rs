//! HTTP proxy frames retain the original bluesky archive layout and action tags.
use rkyv::{Archive, Deserialize, Serialize, rancor::Error, util::AlignedVec};
use tokio_util::bytes::{Buf, Bytes, BytesMut};

pub const MAX_TUNNEL_CHUNK: usize = 8 * 1024;
const HEADER_LEN_PREFIX: usize = size_of::<u64>();
const MAX_HEADER_LEN: usize = 4096;

#[derive(Archive, Serialize, Deserialize)]
enum SessionType {
    SessionHeader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardProtocol {
    Tcp,
    Udp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Connect((u64, String)),
    FailedEndPoints(Vec<u64>),
    HttpPacket(u64),
    Error(String),
    ConnectUdp((u64, String)),
    UdpPacket(u64),
    Connected(u64),
    Close(u64),
    ShutdownWrite(u64),
}

impl Action {
    fn valid_payload(&self, size: usize) -> bool {
        match self {
            Self::HttpPacket(_) => size != 0,
            Self::UdpPacket(_) => true,
            _ => size == 0,
        }
    }
}

// Retain reserved tags AND their archive sizes: removing them changes rkyv's
// enum layout and breaks existing HTTP clients. They have no runtime handlers.
#[derive(Archive, Serialize, Deserialize)]
enum WireAction {
    Connect((u64, String)),
    FailedEndPoints(Vec<u64>),
    Reserved2,
    Reserved3((Option<Vec<u8>>, Option<Vec<u8>>)),
    Reserved4,
    HttpPacket(u64),
    Error(String),
    ConnectUdp((u64, String)),
    UdpPacket(u64),
    Connected(u64),
    Close(u64),
    ShutdownWrite(u64),
}

impl From<Action> for WireAction {
    fn from(action: Action) -> Self {
        match action {
            Action::Connect(dest) => Self::Connect(dest),
            Action::FailedEndPoints(ids) => Self::FailedEndPoints(ids),
            Action::HttpPacket(id) => Self::HttpPacket(id),
            Action::Error(error) => Self::Error(error),
            Action::ConnectUdp(dest) => Self::ConnectUdp(dest),
            Action::UdpPacket(id) => Self::UdpPacket(id),
            Action::Connected(id) => Self::Connected(id),
            Action::Close(id) => Self::Close(id),
            Action::ShutdownWrite(id) => Self::ShutdownWrite(id),
        }
    }
}

#[derive(Archive, Serialize, Deserialize)]
struct SessionHeader {
    s_type: SessionType,
    action: WireAction,
    payload_len: u16,
}

pub fn encode_frame(action: Action, payload: &[u8]) -> Option<Bytes> {
    if !action.valid_payload(payload.len()) {
        return None;
    }
    let header = SessionHeader {
        s_type: SessionType::SessionHeader,
        action: action.into(),
        payload_len: payload.len().try_into().ok()?,
    };
    let header = rkyv::to_bytes::<Error>(&header).ok()?;
    if header.len() > MAX_HEADER_LEN || payload.len() > MAX_TUNNEL_CHUNK {
        return None;
    }
    let mut frame = BytesMut::with_capacity(HEADER_LEN_PREFIX + header.len() + payload.len());
    frame.extend_from_slice(&(header.len() as u64).to_be_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(payload);
    Some(frame.freeze())
}

pub fn encode_action(action: Action) -> Option<Bytes> {
    encode_frame(action, &[])
}

pub fn decode_frame(mut frame: BytesMut) -> Option<(Action, Option<Bytes>)> {
    let len = u64::from_be_bytes(frame.get(..HEADER_LEN_PREFIX)?.try_into().ok()?);
    let len = usize::try_from(len).ok()?;
    if len > MAX_HEADER_LEN {
        return None;
    }
    let end = HEADER_LEN_PREFIX.checked_add(len)?;
    let mut aligned = AlignedVec::<16>::with_capacity(len);
    aligned.extend_from_slice(frame.get(HEADER_LEN_PREFIX..end)?);
    let header = rkyv::access::<ArchivedSessionHeader, Error>(&aligned).ok()?;
    let action = match rkyv::deserialize::<WireAction, Error>(&header.action).ok()? {
        WireAction::Connect(dest) => Action::Connect(dest),
        WireAction::FailedEndPoints(ids) => Action::FailedEndPoints(ids),
        WireAction::HttpPacket(id) => Action::HttpPacket(id),
        WireAction::Error(error) => Action::Error(error),
        WireAction::ConnectUdp(dest) => Action::ConnectUdp(dest),
        WireAction::UdpPacket(id) => Action::UdpPacket(id),
        WireAction::Connected(id) => Action::Connected(id),
        WireAction::Close(id) => Action::Close(id),
        WireAction::ShutdownWrite(id) => Action::ShutdownWrite(id),
        WireAction::Reserved2 | WireAction::Reserved3(_) | WireAction::Reserved4 => return None,
    };
    let payload_len = header.payload_len.to_native() as usize;
    if !action.valid_payload(payload_len) || payload_len > MAX_TUNNEL_CHUNK {
        return None;
    }
    frame.advance(end);
    if frame.len() != payload_len {
        return None;
    }
    let carries_payload = matches!(action, Action::HttpPacket(_) | Action::UdpPacket(_));
    Some((action, carries_payload.then(|| frame.freeze())))
}

#[cfg(test)]
mod tests;
