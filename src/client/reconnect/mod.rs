//! Scheduled hard session rotation and recovery after transport failures.
use super::ForwardClient;
use crate::config::ReconnectConfig;
use std::{
    future::{Future, pending},
    io,
    time::Duration,
};
use tokio::task::JoinHandle;

pub type ClientSession = (ForwardClient, JoinHandle<io::Result<()>>);

struct ActiveSession(JoinHandle<io::Result<()>>);

impl Drop for ActiveSession {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Serve one authenticated generation at a time. Dropping a generation's serve
/// future must cancel its streams. Existing streams cannot migrate to a new
/// transport. Dropping this supervisor also aborts the active tunnel driver.
/// The connector must release setup resources when its future is cancelled.
pub async fn supervise<C, Connect, S, Serve>(
    reconnect: &ReconnectConfig,
    setup_timeout: Duration,
    mut connect: C,
    mut serve: S,
) -> io::Result<()>
where
    C: FnMut() -> Connect,
    Connect: Future<Output = io::Result<ClientSession>>,
    S: FnMut(ForwardClient) -> Serve,
    Serve: Future<Output = io::Result<()>>,
{
    reconnect
        .validate()
        .map_err(|errors| io::Error::new(io::ErrorKind::InvalidInput, errors.join("; ")))?;
    loop {
        let session = tokio::time::timeout(setup_timeout, connect()).await;
        let (client, driver) = match session {
            Ok(Ok(session)) => session,
            failed => {
                let error = match failed {
                    Ok(Err(error)) => error,
                    Err(error) => error.into(),
                    Ok(Ok(_)) => unreachable!(),
                };
                eprintln!(
                    "Tunnel connection failed: {error}; retrying in {}s",
                    reconnect.retry_delay_secs
                );
                tokio::time::sleep(reconnect.retry_delay()).await;
                continue;
            }
        };
        let mut active = ActiveSession(driver);
        let lifetime = reconnect
            .next_lifetime()
            .map_err(|errors| io::Error::new(io::ErrorKind::InvalidInput, errors.join("; ")))?;
        if let Some(duration) = lifetime {
            eprintln!(
                "Tunnel connected; rotating session in {}s",
                duration.as_secs()
            );
        } else {
            eprintln!("Tunnel connected; scheduled rotation disabled");
        }
        let timer = async {
            match lifetime {
                Some(duration) => tokio::time::sleep(duration).await,
                None => pending::<()>().await,
            }
        };
        let driver_finished = tokio::select! {
            result = serve(client) => return result,
            _ = timer => {
                eprintln!("Session lifetime reached; reconnecting");
                false
            }
            result = &mut active.0 => {
                match result {
                    Ok(Ok(())) => eprintln!("Tunnel closed; reconnecting"),
                    Ok(Err(error)) => eprintln!("Tunnel failed: {error}; reconnecting"),
                    Err(error) => eprintln!("Tunnel driver stopped: {error}; reconnecting"),
                }
                true
            }
        };
        // End the old transport before starting a new handshake on the setup port.
        if driver_finished {
            tokio::time::sleep(reconnect.retry_delay()).await;
        } else {
            active.0.abort();
            let _ = (&mut active.0).await;
        }
    }
}

#[cfg(test)]
mod tests;
