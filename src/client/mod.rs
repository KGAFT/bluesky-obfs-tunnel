//! Client tunnel handles and independent TCP/UDP forwarding connections.
mod connection;
pub mod reconnect;
mod session;
mod tunnel;

pub use crate::protocol::ForwardProtocol;
pub use connection::{ForwardConnection, ForwardReceiver, ForwardSender};
pub use tunnel::ForwardClient;

fn broken_pipe() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "forwarding connection is closed",
    )
}

#[cfg(test)]
mod tests;
