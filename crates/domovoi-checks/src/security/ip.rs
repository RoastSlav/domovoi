//! Whether an address is on the local network or out on the internet.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Where an address lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// Private, loopback, link-local, CGNAT or unique-local: not reachable from the internet.
    Lan,
    /// Anything else.
    Wan,
}

impl Network {
    /// `lan` or `wan`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lan => "lan",
            Self::Wan => "wan",
        }
    }
}

fn v4_is_lan(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        // CGNAT, 100.64.0.0/10, used by Tailscale and carrier NAT.
        || (a == 100 && (b & 0xc0) == 64)
}

fn v6_is_lan(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return v4_is_lan(v4);
    }
    let first = ip.segments()[0];
    ip.is_loopback()
        || ip.is_unspecified()
        // Unique local, fc00::/7.
        || (first & 0xfe00) == 0xfc00
        // Link-local, fe80::/10.
        || (first & 0xffc0) == 0xfe80
}

/// Classifies `ip`. IPv4-mapped IPv6 addresses are judged as IPv4.
#[must_use]
pub fn classify_ip(ip: IpAddr) -> Network {
    let lan = match ip {
        IpAddr::V4(v4) => v4_is_lan(v4),
        IpAddr::V6(v6) => v6_is_lan(v6),
    };
    if lan { Network::Lan } else { Network::Wan }
}
