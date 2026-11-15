//! Messages from public client handles to the tunnel driver.
use crate::protocol::ForwardProtocol;
use std::{io, sync::Arc};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::bytes::Bytes;

pub(super) type Reply = oneshot::Sender<io::Result<()>>;

pub(crate) enum Command {
    Open {
        id: u64,
        protocol: ForwardProtocol,
        destination: String,
        incoming: mpsc::Sender<Bytes>,
        error: watch::Sender<Option<Arc<io::Error>>>,
        ready: Reply,
    },
    Data {
        id: u64,
        data: Bytes,
        reply: Reply,
    },
    ShutdownWrite {
        id: u64,
        reply: Reply,
    },
    Close {
        id: u64,
        reply: Reply,
    },
    Stop(Reply),
}
