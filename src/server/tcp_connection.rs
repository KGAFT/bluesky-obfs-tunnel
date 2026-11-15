use std::net::IpAddr;
use tokio::net::TcpStream;

pub struct TcpConnection {
    addr: IpAddr,
    port: u16,
}

impl TcpConnection {
    pub fn new(addr: IpAddr, port: u16) -> Self {
        Self { addr, port }
    }

    pub async fn instantiate(&self) -> Result<TcpStream, std::io::Error> {
        TcpStream::connect((self.addr, self.port)).await
    }
}
