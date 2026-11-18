//! Command-line arguments and usage text.
use super::invalid;
use bluesky_obfs_tunnel::client::ForwardProtocol;
use bluesky_obfs_tunnel::config::ClientSettings;
use std::{io, net::SocketAddr, time::Duration};

pub(super) const HELP: &str = "Usage: client [OPTIONS]

Send a test message through the example obfuscated tunnel and print the reply.
With no destination option, use the server's default fixed TCP forwarding mode.
--tcp, --udp and --http-proxy require a server started with --multiplexed.

Options:
  --config PATH          Load/create JSON settings (default: client.json)
  --server HOST:PORT     Override the configured tunnel server
  --tcp DEST[:LOCAL]     TCP destination HOST:PORT, optionally followed by :IP:PORT
  --udp DEST[:LOCAL]     UDP destination HOST:PORT, optionally followed by :IP:PORT
  --http-proxy ADDR      Listen as an HTTP/HTTPS CONNECT proxy (e.g. 127.0.0.1:8080)
  --proxy                Listen on the configured local_proxy_bind address
  --message TEXT         Payload (default: hello from client followed by newline)
  --file PATH            Read a binary payload from a file instead of --message
  --timeout SECONDS      Override request/proxy setup timeout
  --setup-port PORT      Override the local handshake proxy port
  -h, --help             Show this help

Without a local address, TCP sends a write half-close and reads until EOF;
UDP sends exactly one datagram. With a local address, forward continuously.
Proxy and forwarding listeners run until Ctrl-C and reconnect automatically.
Examples: client --tcp 127.0.0.1:8090:127.0.0.1:1080
          client --udp 127.0.0.1:5353:127.0.0.1:1053
Destinations are reached from the tunnel server. Use brackets around IPv6 hosts.
UDP preserves datagrams up to 8192 bytes; idle sender mappings expire after 60s.
Use the same cli_pattern.bin and serv_pattern.bin as the example server.
Credentials, cover site, queues, and session lifetimes are loaded from JSON.
";

#[derive(Debug)]
pub(super) struct Options {
    pub(super) config: String,
    pub(super) server: Option<String>,
    pub(super) destination: Option<(ForwardProtocol, String)>,
    pub(super) listen: Option<SocketAddr>,
    pub(super) http_proxy: Option<String>,
    pub(super) proxy: bool,
    pub(super) message: Option<String>,
    pub(super) file: Option<String>,
    pub(super) timeout: Option<Duration>,
    pub(super) setup_port: Option<u16>,
}

impl Options {
    pub(super) fn parse(args: impl IntoIterator<Item = String>) -> io::Result<Option<Self>> {
        let mut options = Self {
            config: "client.json".into(),
            server: None,
            destination: None,
            listen: None,
            http_proxy: None,
            proxy: false,
            message: None,
            file: None,
            timeout: None,
            setup_port: None,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--help" || arg == "-h" {
                return Ok(None);
            }
            if arg == "--proxy" {
                options.proxy = true;
                continue;
            }
            if let Some(path) = arg.strip_prefix("--config=") {
                if path.is_empty() {
                    return Err(invalid("--config requires a path"));
                }
                options.config = path.into();
                continue;
            }
            if !matches!(
                arg.as_str(),
                "--server"
                    | "--config"
                    | "--tcp"
                    | "--udp"
                    | "--http-proxy"
                    | "--message"
                    | "--file"
                    | "--timeout"
                    | "--setup-port"
            ) {
                return Err(invalid(format!("unknown argument: {arg}; use --help")));
            }
            let value = args
                .next()
                .filter(|value| {
                    !value.starts_with("--") && (arg != "--config" || !value.is_empty())
                })
                .ok_or_else(|| invalid(format!("{arg} requires a value")))?;
            match arg.as_str() {
                "--config" => options.config = value,
                "--server" => options.server = Some(value),
                "--tcp" | "--udp" => {
                    if options.destination.is_some() {
                        return Err(invalid("choose one --tcp or --udp destination"));
                    }
                    let protocol = if arg == "--tcp" {
                        ForwardProtocol::Tcp
                    } else {
                        ForwardProtocol::Udp
                    };
                    let (destination, listen) = forwarding_addresses(&arg, &value)?;
                    options.destination = Some((protocol, destination));
                    options.listen = listen;
                }
                "--message" => options.message = Some(value),
                "--http-proxy" => {
                    if options.http_proxy.replace(value).is_some() {
                        return Err(invalid("choose one --http-proxy listener"));
                    }
                }
                "--file" => options.file = Some(value),
                "--timeout" => {
                    let seconds = value
                        .parse::<u32>()
                        .ok()
                        .filter(|seconds| *seconds > 0)
                        .ok_or_else(|| invalid("--timeout must be a positive integer"))?;
                    options.timeout = Some(Duration::from_secs(u64::from(seconds)));
                }
                "--setup-port" => {
                    options.setup_port = Some(
                        value
                            .parse::<u16>()
                            .ok()
                            .filter(|port| *port > 0)
                            .ok_or_else(|| invalid("--setup-port must be between 1 and 65535"))?,
                    );
                }
                _ => unreachable!(),
            }
        }
        if options.message.is_some() && options.file.is_some() {
            return Err(invalid("--message and --file are mutually exclusive"));
        }
        if options.is_proxy()
            && (options.destination.is_some()
                || options.message.is_some()
                || options.file.is_some())
        {
            return Err(invalid(
                "--http-proxy cannot be combined with --tcp, --udp, --message or --file",
            ));
        }
        if options.listen.is_some()
            && (options.is_proxy() || options.message.is_some() || options.file.is_some())
        {
            return Err(invalid(
                "continuous forwarding cannot be combined with proxy or payload options",
            ));
        }
        Ok(Some(options))
    }

    pub(super) fn is_proxy(&self) -> bool {
        self.proxy || self.http_proxy.is_some()
    }

    pub(super) async fn load_settings(&self) -> io::Result<ClientSettings> {
        let mut settings = ClientSettings::load_or_create(&self.config).await?;
        self.apply(&mut settings)?;
        Ok(settings)
    }

    pub(super) fn apply(&self, settings: &mut ClientSettings) -> io::Result<()> {
        if let Some(server) = &self.server {
            settings.server_address = server.clone();
        }
        if let Some(bind) = &self.http_proxy {
            settings.local_proxy_bind = bind.clone();
        }
        if let Some(timeout) = self.timeout {
            settings.proxy_setup_timeout_secs = timeout.as_secs();
        }
        if let Some(port) = self.setup_port {
            settings.obfuscation.setup_proxy_port = port;
        }
        settings
            .validate()
            .map_err(|problems| invalid(problems.join("; ")))
    }
}

/// Split after the destination port, ignoring colons inside bracketed IPv6 hosts.
fn forwarding_addresses(flag: &str, value: &str) -> io::Result<(String, Option<SocketAddr>)> {
    let invalid_address = || {
        invalid(format!(
            "{flag} must be HOST:PORT or HOST:PORT:IP:PORT (IPv6 hosts need brackets)"
        ))
    };
    let host_end = if value.starts_with('[') {
        value.find(']').map(|index| index + 1)
    } else {
        value.find(':')
    }
    .ok_or_else(invalid_address)?;
    let port_and_local = value[host_end..]
        .strip_prefix(':')
        .ok_or_else(invalid_address)?;
    let (port, local) = match port_and_local.split_once(':') {
        Some((port, local)) => (port, Some(local)),
        None => (port_and_local, None),
    };
    if !port.parse::<u16>().is_ok_and(|port| port != 0) {
        return Err(invalid_address());
    }
    let destination = &value[..host_end + 1 + port.len()];
    if value.starts_with('[') {
        destination
            .parse::<SocketAddr>()
            .map_err(|_| invalid_address())?;
    } else if host_end == 0
        || value[..host_end]
            .chars()
            .any(|c| c.is_whitespace() || ":/[]@".contains(c))
    {
        return Err(invalid_address());
    }
    let listen = local
        .map(|local| local.parse::<SocketAddr>().map_err(|_| invalid_address()))
        .transpose()?;
    Ok((destination.to_owned(), listen))
}

#[cfg(test)]
mod tests;
