use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::lookup_host;

#[derive(Clone, Debug)]
pub struct ConnectPolicy {
    allow_local: bool,
    allowlist: Arc<[String]>,
    /// The host's own public addresses, which reach its published ports.
    public_ips: Arc<[IpAddr]>,
}

impl Default for ConnectPolicy {
    fn default() -> Self {
        Self {
            allow_local: true,
            allowlist: Arc::from([]),
            public_ips: Arc::from([]),
        }
    }
}

impl ConnectPolicy {
    pub fn new(allow_local: bool, allowlist: &[String], public_ips: &[IpAddr]) -> Self {
        Self {
            allow_local,
            allowlist: allowlist
                .iter()
                .map(|d| d.trim().to_ascii_lowercase())
                .collect(),
            public_ips: public_ips.iter().map(IpAddr::to_canonical).collect(),
        }
    }

    fn is_forbidden(&self, ip: IpAddr) -> bool {
        is_local(ip) || self.public_ips.contains(&ip.to_canonical())
    }

    fn is_allowlisted(&self, dest: &str) -> bool {
        let dest = dest.trim().to_ascii_lowercase();
        self.allowlist.contains(&dest)
    }

    pub async fn resolve(&self, dest: &str) -> io::Result<Vec<SocketAddr>> {
        let addrs: Vec<SocketAddr> = lookup_host(dest).await?.collect();
        if self.allow_local || self.is_allowlisted(dest) {
            return Ok(addrs);
        }

        let allowed: Vec<SocketAddr> = addrs
            .iter()
            .copied()
            .filter(|a| !self.is_forbidden(a.ip()))
            .collect();
        if allowed.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{dest} resolves only to local addresses {addrs:?}"),
            ));
        }
        Ok(allowed)
    }
}
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_local_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_local_v4(v4),
            None => is_local_v6(v6),
        },
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (b & 0xc0) == 64)
}

fn is_local_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
}
