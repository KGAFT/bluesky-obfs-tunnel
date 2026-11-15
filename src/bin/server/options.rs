use bluesky_obfs_tunnel::config::{ForwardMode, ServerSettings};
use std::io;

pub(super) const HELP: &str = "Usage: bluesky-obfs-tunnel [OPTIONS]

  --config PATH    Load/create server JSON settings (default: server.json)
  --bind IP:PORT   Override the tunnel listener (default: 0.0.0.0:443)
  --tcp IP:PORT    Forward raw TCP to a fixed destination (default: 127.0.0.1:2080)
  --multiplexed    Override forwarding.mode to multiplexed
  --create-client ID  Create a database client using CLIENT_PASSWORD, then exit
  -h, --help       Show this help

CLI overrides are not saved to the configuration file. Restart to apply changes.
";

pub(super) struct Options {
    pub(super) config: String,
    pub(super) multiplexed: bool,
    pub(super) bind: Option<String>,
    pub(super) tcp: Option<std::net::SocketAddr>,
    pub(super) create_client: Option<String>,
}

impl Options {
    pub(super) fn parse(args: impl IntoIterator<Item = String>) -> io::Result<Option<Self>> {
        let mut options = Self {
            config: "server.json".into(),
            multiplexed: false,
            bind: None,
            tcp: None,
            create_client: None,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--multiplexed" => options.multiplexed = true,
                "--create-client" => {
                    let value = args
                        .next()
                        .filter(|arg| !arg.starts_with("--") && !arg.trim().is_empty())
                        .ok_or_else(|| invalid("--create-client requires an identity"))?;
                    if options.create_client.replace(value).is_some() {
                        return Err(invalid("choose one --create-client identity"));
                    }
                }
                "--bind" | "--tcp" => {
                    let value = args
                        .next()
                        .ok_or_else(|| invalid(format!("{arg} requires IP:PORT")))?;
                    let address = value
                        .parse::<std::net::SocketAddr>()
                        .map_err(|_| invalid(format!("{arg} must be IP:PORT")))?;
                    if arg == "--bind" {
                        if options.bind.replace(value).is_some() {
                            return Err(invalid("choose one --bind address"));
                        }
                    } else {
                        if address.port() == 0 {
                            return Err(invalid("--tcp destination port must be at least 1"));
                        }
                        if options.tcp.replace(address).is_some() {
                            return Err(invalid("choose one --tcp destination"));
                        }
                    }
                }
                "--config" => {
                    options.config = args
                        .next()
                        .filter(|arg| !arg.starts_with("--") && !arg.is_empty())
                        .ok_or_else(|| invalid("--config requires a path"))?
                }
                _ => {
                    if let Some(path) = arg
                        .strip_prefix("--config=")
                        .filter(|path| !path.is_empty())
                    {
                        options.config = path.into();
                    } else {
                        return Err(invalid(format!("unknown argument: {arg}; use --help")));
                    }
                }
            }
        }
        if options.multiplexed && options.tcp.is_some() {
            return Err(invalid("--tcp and --multiplexed are mutually exclusive"));
        }
        if options.create_client.is_some()
            && (options.multiplexed || options.tcp.is_some() || options.bind.is_some())
        {
            return Err(invalid(
                "--create-client cannot be combined with forwarding overrides",
            ));
        }
        Ok(Some(options))
    }

    pub(super) async fn load_settings(&self) -> io::Result<ServerSettings> {
        let mut settings = ServerSettings::load_or_create(&self.config).await?;
        self.apply(&mut settings)?;
        Ok(settings)
    }

    fn apply(&self, settings: &mut ServerSettings) -> io::Result<()> {
        if let Some(bind) = &self.bind {
            settings.bind_address = bind.clone();
        }
        if let Some(destination) = self.tcp {
            settings.forwarding = ForwardMode::Tcp { destination };
        }
        if self.multiplexed {
            settings.forwarding = ForwardMode::Multiplexed;
        }
        settings
            .validate()
            .map_err(|problems| invalid(problems.join("; ")))
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[cfg(test)]
mod tests;
