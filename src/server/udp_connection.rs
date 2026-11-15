use std::net::IpAddr;
use tokio::net::UdpSocket;

pub struct UdpConnection {
    addr: IpAddr,
    port: u16,
}

impl UdpConnection {
    pub fn new(addr: IpAddr, port: u16) -> Self {
        Self { addr, port }
    }

    pub async fn instantiate(&self) -> Result<UdpSocket, std::io::Error> {
        let bind = if self.addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(bind).await?;
        socket.connect((self.addr, self.port)).await?;
        Ok(socket)
    }
}
