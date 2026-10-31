//! `probe.gateway`: the default gateway answers.
//!
//! - **Source:** the default route in `/proc/1/net/route` (falling back to
//!   `/proc/net/route`), then TCP connections to the gateway on ports 53, 80 and 443 at
//!   once. Any answer counts, including a refused connection, since a refusal comes from
//!   the gateway itself.
//! - **Target:** none. **Severity:** warning when there is no default route or the gateway
//!   does not answer.
//! - **Evidence:** `gateway`, `interface`.
//!
//! [`gateway_reachable`] is the same test as a plain function, for the hub's self-check.

use std::io::ErrorKind;
use std::net::Ipv4Addr;

use async_trait::async_trait;
use domovoi_core::ports::{Clock, HostFs};
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::tcp::connect;
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, key};

/// The ports tried on the gateway.
pub const GATEWAY_PORTS: [u16; 3] = [53, 80, 443];
/// How long each connection attempt may take.
pub const GATEWAY_TIMEOUT: SignedDuration = SignedDuration::from_secs(3);

const RTF_GATEWAY: u32 = 0x2;

/// The default route: its interface and gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultRoute {
    /// The interface, such as `eth0`.
    pub interface: String,
    /// The gateway address.
    pub gateway: Ipv4Addr,
}

/// The default route with the lowest metric in `/proc/net/route`.
#[must_use]
pub fn parse_default_gateway(route: &str) -> Option<DefaultRoute> {
    let mut best: Option<(u32, DefaultRoute)> = None;
    for line in route.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let (Some(iface), Some(dest), Some(gw), Some(flags), Some(metric)) = (
            fields.first(),
            fields.get(1),
            fields.get(2),
            fields.get(3),
            fields.get(6),
        ) else {
            continue;
        };
        let Ok(flags) = u32::from_str_radix(flags, 16) else {
            continue;
        };
        if *dest != "00000000" || flags & RTF_GATEWAY == 0 {
            continue;
        }
        let Ok(gw) = u32::from_str_radix(gw, 16) else {
            continue;
        };
        let metric: u32 = metric.parse().unwrap_or(u32::MAX);
        let route = DefaultRoute {
            interface: (*iface).to_owned(),
            gateway: Ipv4Addr::from(gw.to_le_bytes()),
        };
        if best.as_ref().is_none_or(|(m, _)| metric < *m) {
            best = Some((metric, route));
        }
    }
    best.map(|(_, route)| route)
}

/// What [`gateway_status`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayStatus {
    /// There is no default route.
    NoRoute,
    /// The gateway answered.
    Reachable(DefaultRoute),
    /// The gateway did not answer on any port.
    Unreachable(DefaultRoute),
}

async fn answers(gateway: Ipv4Addr, port: u16, clock: &dyn Clock) -> bool {
    match connect(&gateway.to_string(), port, clock, GATEWAY_TIMEOUT).await {
        Ok(()) => true,
        Err(err) => err.kind() == ErrorKind::ConnectionRefused,
    }
}

/// Finds the default gateway and tries to reach it.
pub async fn gateway_status(fs: &dyn HostFs, clock: &dyn Clock) -> GatewayStatus {
    let text = match fs.read_to_string("/proc/1/net/route").await {
        Ok(text) => text,
        Err(_) => fs
            .read_to_string("/proc/net/route")
            .await
            .unwrap_or_default(),
    };
    let Some(route) = parse_default_gateway(&text) else {
        return GatewayStatus::NoRoute;
    };
    let gw = route.gateway;
    let [a, b, c] = GATEWAY_PORTS;
    let (x, y, z) = tokio::join!(
        answers(gw, a, clock),
        answers(gw, b, clock),
        answers(gw, c, clock)
    );
    if x || y || z {
        GatewayStatus::Reachable(route)
    } else {
        GatewayStatus::Unreachable(route)
    }
}

/// Whether the default gateway answers on port 53, 80 or 443.
pub async fn gateway_reachable(fs: &dyn HostFs, clock: &dyn Clock) -> bool {
    matches!(gateway_status(fs, clock).await, GatewayStatus::Reachable(_))
}

/// The `probe.gateway` check.
pub struct GatewayProbe {
    key: CheckKey,
}

impl GatewayProbe {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("probe.gateway")?,
        })
    }
}

fn with_route(obs: Observation, route: &DefaultRoute) -> Observation {
    obs.with_text("gateway", route.gateway.to_string())
        .with_text("interface", route.interface.clone())
}

#[async_trait]
impl Check for GatewayProbe {
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
        let observation = match gateway_status(ctx.fs, ctx.clock).await {
            GatewayStatus::NoRoute => {
                Observation::firing(None, Severity::Warning, "no default route")
            }
            GatewayStatus::Reachable(route) => with_route(
                Observation::ok(None, format!("gateway {} answers", route.gateway)),
                &route,
            ),
            GatewayStatus::Unreachable(route) => with_route(
                Observation::firing(
                    None,
                    Severity::Warning,
                    format!("gateway {} does not answer", route.gateway),
                ),
                &route,
            ),
        };
        Ok(CheckReport {
            observations: vec![observation],
            facts: Vec::new(),
        })
    }
}
