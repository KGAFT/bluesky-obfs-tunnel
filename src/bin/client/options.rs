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
  --tcp HOST:PORT        Open a multiplexed TCP destination
  --listen IP:PORT       Forward local TCP connections to --tcp HOST:PORT
  --udp HOST:PORT        Open a multiplexed UDP destination; receive one datagram
  --http-proxy ADDR      Listen as an HTTP/HTTPS CONNECT proxy (e.g. 127.0.0.1:8080)
  --proxy                Listen on the configured local_proxy_bind address
  --message TEXT         Payload (default: hello from client followed by newline)
  --file PATH            Read a binary payload from a file instead of --message
  --timeout SECONDS      Override request/proxy setup timeout
  --setup-port PORT      Override the local handshake proxy port
  -h, --help             Show this help

TCP sends a write half-close and reads until EOF. UDP sends exactly one datagram.
Proxy and --listen modes run until Ctrl-C; sessions rotate per reconnect settings.
Example: client --listen 127.0.0.1:1080 --tcp 127.0.0.1:2080
The TCP destination is reached from the tunnel server.
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
                    | "--listen"
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
                "--listen" => {
                    let address = value
                        .parse::<SocketAddr>()
                        .map_err(|_| invalid("--listen must be IP:PORT"))?;
                    if options.listen.replace(address).is_some() {
                        return Err(invalid("choose one --listen address"));
                    }
                }
                "--tcp" | "--udp" => {
                    if options.destination.is_some() {
                        return Err(invalid("choose one --tcp or --udp destination"));
                    }
                    let protocol = if arg == "--tcp" {
                        ForwardProtocol::Tcp
                    } else {
                        ForwardProtocol::Udp
                    };
                    options.destination = Some((protocol, value));
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
            && (options.is_proxy()
                || !matches!(options.destination, Some((ForwardProtocol::Tcp, _)))
                || options.message.is_some()
                || options.file.is_some())
        {
            return Err(invalid(
                "--listen requires --tcp and cannot be combined with proxy or payload options",
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
