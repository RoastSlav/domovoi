//! Network probes: TCP, HTTP and the default gateway.
//!
//! Probes are checks like any other, but the hub builds them itself from `[[probes]]`, one
//! per target, so they are not in the agent's registry. Every timeout races the clock, so
//! tests never wait on real time.

pub mod cert;
pub mod gateway;
pub mod http;
pub mod tcp;

use jiff::Timestamp;

pub use gateway::{
    DefaultRoute, GatewayProbe, GatewayStatus, gateway_reachable, gateway_status,
    parse_default_gateway,
};
pub use http::HttpProbe;
pub use tcp::{HostPort, TcpProbe};

/// Why a probe could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    /// The target is not something the probe can reach.
    #[error("invalid probe target: {0}")]
    InvalidTarget(String),
    /// The TLS client could not be set up.
    #[error("setting up TLS: {0}")]
    Tls(String),
}

/// Milliseconds between `start` and `end`.
pub(crate) fn elapsed_ms(start: Timestamp, end: Timestamp) -> f64 {
    end.duration_since(start).as_secs_f64() * 1000.0
}
