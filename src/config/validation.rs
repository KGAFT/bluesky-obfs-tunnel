use std::{io, net::SocketAddr};

pub(super) fn result(problems: Vec<String>) -> Result<(), Vec<String>> {
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

pub(super) fn invalid(problems: Vec<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, problems.join("; "))
}

pub(super) fn host_port(value: &str) -> bool {
    if let Ok(address) = value.parse::<SocketAddr>() {
        return address.port() != 0;
    }
    let Some((host, port)) = value.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && !host
            .chars()
            .any(|c| c.is_whitespace() || ":/[]@".contains(c))
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

pub(super) fn nonempty(name: &str, value: &str, problems: &mut Vec<String>) {
    if value.trim().is_empty() {
        problems.push(format!("{name} must be set"));
    }
}

pub(super) fn positive(name: &str, value: u64, problems: &mut Vec<String>) {
    if value == 0 {
        problems.push(format!("{name} must be at least 1"));
    }
}

pub(super) fn ratio(name: &str, value: f64, problems: &mut Vec<String>) {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        problems.push(format!("{name} must be finite and between 0 and 1"));
    }
}
