//! `probe.tcp`: a TCP port accepts connections.
//!
//! - **Target:** `host:port`. **Severity:** warning, or critical when the probe's
//!   `criticality` is critical.
//! - The connection must open within [`TCP_TIMEOUT`] as measured by the clock.
//! - **Evidence:** `latency_ms` when up, `error` when down.

use std::fmt;

use async_trait::async_trait;
use domovoi_core::ports::Clock;
use domovoi_core::{CheckKey, Criticality, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;
use tokio::net::TcpStream;

use super::{ProbeError, elapsed_ms};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, key, target};

/// How long a connection may take to open.
pub const TCP_TIMEOUT: SignedDuration = SignedDuration::from_secs(5);

/// A validated `host:port`, where host is a DNS name or an IP address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPort {
    host: String,
    port: u16,
}

impl HostPort {
    /// Parses `host:port` or `[v6]:port`.
    pub fn parse(input: &str) -> Result<Self, ProbeError> {
        let bad = || ProbeError::InvalidTarget(format!("{input:?} is not host:port"));
        let (host, port) = input.rsplit_once(':').ok_or_else(bad)?;
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        let port: u16 = port.parse().map_err(|_| bad())?;
        let is_ip = host.parse::<std::net::IpAddr>().is_ok();
        let is_name = !host.is_empty()
            && host.len() <= 253
            && !host.starts_with('-')
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
        if port == 0 || !(is_ip || is_name) {
            return Err(bad());
        }
        Ok(Self {
            host: host.to_owned(),
            port,
        })
    }

    /// The host.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl fmt::Display for HostPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') {
            write!(f, "[{}]:{}", self.host, self.port)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

/// Opens a TCP connection to `host:port`, giving up after `timeout` on `clock`.
pub async fn connect(
    host: &str,
    port: u16,
    clock: &dyn Clock,
    timeout: SignedDuration,
) -> std::io::Result<()> {
    let now = clock.now();
    let deadline = now.checked_add(timeout).unwrap_or(now);
    tokio::select! {
        result = TcpStream::connect((host, port)) => result.map(drop),
        () = clock.sleep_until(deadline) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("no answer within {}s", timeout.as_secs()),
        )),
    }
}

/// The `probe.tcp` check for one target.
pub struct TcpProbe {
    key: CheckKey,
    target: HostPort,
}

impl TcpProbe {
    /// The probe of `target`.
    pub fn new(target: HostPort) -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("probe.tcp")?,
            target,
        })
    }
}

#[async_trait]
impl Check for TcpProbe {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let name = self.target.to_string();
        let start = ctx.clock.now();
        let result = connect(&self.target.host, self.target.port, ctx.clock, TCP_TIMEOUT).await;
        let observation = match result {
            Ok(()) => Observation::ok(target(&name), format!("{name} accepts connections"))
                .with_number("latency_ms", elapsed_ms(start, ctx.clock.now())),
            Err(err) => {
                let severity = match ctx.settings.criticality {
                    Some(Criticality::Critical) => Severity::Critical,
                    _ => Severity::Warning,
                };
                Observation::firing(target(&name), severity, format!("{name} is down: {err}"))
                    .with_text("error", err.to_string())
            }
        };
        Ok(CheckReport {
            observations: vec![observation],
            facts: Vec::new(),
        })
    }
}
