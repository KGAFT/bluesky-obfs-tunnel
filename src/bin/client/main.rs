//! Example tunnel client command.
mod app;
mod config;
mod http_proxy;
mod options;
mod tcp_forward;
mod transfer;
mod udp_forward;

use app::{run, run_proxy};
use options::{HELP, Options};
use std::{io, process::ExitCode};

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = async {
        let Some(options) = Options::parse(std::env::args().skip(1))? else {
            print!("{HELP}");
            return Ok(());
        };
        let settings = options.load_settings().await?;
        if options.is_proxy() {
            return run_proxy(settings).await;
        }
        if let Some(bind) = options.listen {
            let (protocol, destination) = options
                .destination
                .expect("validated forwarding destination");
            return app::run_forward(settings, protocol, bind, destination).await;
        }
        tokio::time::timeout(settings.proxy_setup_timeout(), run(options, settings))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "client request timed out"))?
    }
    .await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("client: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
