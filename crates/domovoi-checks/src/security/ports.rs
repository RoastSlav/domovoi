//! `security.new_port`: something started listening on a new port.
//!
//! - **Source:** `/proc/1/net/{tcp,tcp6,udp,udp6}` (the host's network namespace, also in
//!   container mode), falling back to `/proc/net/*`. TCP sockets in `LISTEN` count; UDP
//!   sockets count when they are unconnected and bound outside the ephemeral range
//!   (`/proc/sys/net/ipv4/ip_local_port_range`), so client sockets are ignored.
//! - **No baseline:** every non-loopback listener is reported on every run. Which ones are
//!   fine is decided on the hub, in its known conditions; nothing is learned here. The
//!   first run marks its observations `first_run`, so the hub can record them as one
//!   reviewable group. Loopback listeners are ignored.
//! - **Target:** `addr:port/proto`, such as `0.0.0.0:22/tcp` or `[::]:443/tcp`.
//!   **Severity:** warning. **Class:** immediate.
//! - **Evidence:** `address`, `port`, `proto`; `first_run` on the first run.

use std::collections::BTreeSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use async_trait::async_trait;
use domovoi_core::ports::HostFs;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, key, target};

/// The state key that says this agent has reported its listeners once.
pub const REPORTED_STATE: &str = "reported";
/// Linux's default ephemeral port range.
pub const DEFAULT_EPHEMERAL: (u16, u16) = (32768, 60999);
/// Most listeners read from one file.
pub const MAX_LISTENERS: usize = 4096;

/// A transport protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Proto {
    /// TCP.
    Tcp,
    /// UDP.
    Udp,
}

impl Proto {
    /// `tcp` or `udp`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

/// One listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Listener {
    /// The bound address.
    pub addr: IpAddr,
    /// The bound port.
    pub port: u16,
    /// The protocol.
    pub proto: Proto,
}

impl fmt::Display for Listener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.addr {
            IpAddr::V4(a) => write!(f, "{a}:{}/{}", self.port, self.proto.as_str()),
            IpAddr::V6(a) => write!(f, "[{a}]:{}/{}", self.port, self.proto.as_str()),
        }
    }
}

/// Decodes a `/proc/net` address: hex words in host (little-endian) byte order.
fn decode_addr(hex: &str) -> Option<IpAddr> {
    let word = |chunk: &str| u32::from_str_radix(chunk, 16).ok().map(u32::to_le_bytes);
    match hex.len() {
        8 => Some(IpAddr::V4(Ipv4Addr::from(word(hex)?))),
        32 => {
            let mut bytes = [0_u8; 16];
            for (i, out) in bytes.chunks_exact_mut(4).enumerate() {
                out.copy_from_slice(&word(hex.get(i * 8..i * 8 + 8)?)?);
            }
            Some(IpAddr::V6(Ipv6Addr::from(bytes)))
        }
        _ => None,
    }
}

fn decode_endpoint(field: &str) -> Option<(IpAddr, u16)> {
    let (addr, port) = field.split_once(':')?;
    Some((decode_addr(addr)?, u16::from_str_radix(port, 16).ok()?))
}

/// Parses one `/proc/net/{tcp,tcp6,udp,udp6}` file into its listening sockets.
#[must_use]
pub fn parse_proc_net(text: &str, proto: Proto) -> Vec<Listener> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let mut fields = line.split_whitespace().skip(1);
        let (Some(local), Some(remote), Some(state)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let Some((addr, port)) = decode_endpoint(local) else {
            continue;
        };
        let listening = match proto {
            Proto::Tcp => state == "0A",
            Proto::Udp => state == "07" && decode_endpoint(remote).is_some_and(|(_, p)| p == 0),
        };
        if listening && port != 0 {
            out.push(Listener { addr, port, proto });
        }
        if out.len() >= MAX_LISTENERS {
            break;
        }
    }
    out
}

/// Parses `ip_local_port_range`, such as `32768\t60999`.
#[must_use]
pub fn parse_port_range(text: &str) -> Option<(u16, u16)> {
    let mut parts = text.split_whitespace().map(str::parse::<u16>);
    match (parts.next(), parts.next()) {
        (Some(Ok(low)), Some(Ok(high))) if low <= high => Some((low, high)),
        _ => None,
    }
}

/// `[checks."security.new_port"]` thresholds. The check keeps no baseline of its own any more,
/// so `expiry_days` is accepted for an existing config and ignored.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PortThresholds {
    /// Ignored; the expected listeners are the hub's known conditions.
    pub expiry_days: u32,
}

impl Default for PortThresholds {
    fn default() -> Self {
        Self { expiry_days: 7 }
    }
}

async fn read_net(fs: &dyn HostFs, file: &str) -> Option<String> {
    match fs.read_to_string(&format!("/proc/1/net/{file}")).await {
        Ok(text) => Some(text),
        Err(_) => fs.read_to_string(&format!("/proc/net/{file}")).await.ok(),
    }
}

/// Every non-loopback listener on the host.
pub async fn listeners(fs: &dyn HostFs) -> BTreeSet<Listener> {
    let (low, high) = match fs
        .read_to_string("/proc/sys/net/ipv4/ip_local_port_range")
        .await
    {
        Ok(text) => parse_port_range(&text).unwrap_or(DEFAULT_EPHEMERAL),
        Err(_) => DEFAULT_EPHEMERAL,
    };
    let mut out = BTreeSet::new();
    for (file, proto) in [
        ("tcp", Proto::Tcp),
        ("tcp6", Proto::Tcp),
        ("udp", Proto::Udp),
        ("udp6", Proto::Udp),
    ] {
        let Some(text) = read_net(fs, file).await else {
            continue;
        };
        out.extend(parse_proc_net(&text, proto).into_iter().filter(|l| {
            let loopback = match l.addr {
                IpAddr::V4(a) => a.is_loopback(),
                IpAddr::V6(a) => {
                    a.is_loopback() || a.to_ipv4_mapped().is_some_and(|v| v.is_loopback())
                }
            };
            let ephemeral = l.proto == Proto::Udp && (low..=high).contains(&l.port);
            !loopback && !ephemeral
        }));
    }
    out
}

/// The `security.new_port` check.
pub struct NewPorts {
    key: CheckKey,
}

impl NewPorts {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("security.new_port")?,
        })
    }
}

#[async_trait]
impl Check for NewPorts {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Immediate
    }

    fn resolves_itself(&self) -> bool {
        false
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let _: PortThresholds = ctx.settings.thresholds()?;
        let current = listeners(ctx.fs).await;
        // The first run on this agent marks what it finds, so the hub can record the whole
        // group as one reviewable list of expected listeners instead of one alert each.
        let first_run = ctx.state.get(REPORTED_STATE).is_none();
        let mut report = CheckReport::default();
        for listener in &current {
            let name = listener.to_string();
            let mut observation = Observation::firing(
                target(&name),
                Severity::Warning,
                format!("new listener on {name}"),
            )
            .with_text("address", listener.addr.to_string())
            .with("port", EvidenceValue::Int(i64::from(listener.port)))
            .with_text("proto", listener.proto.as_str());
            if first_run {
                observation = observation.with("first_run", EvidenceValue::Bool(true));
            }
            report.observations.push(observation);
        }
        ctx.state.set(REPORTED_STATE, "1");
        Ok(report)
    }
}
