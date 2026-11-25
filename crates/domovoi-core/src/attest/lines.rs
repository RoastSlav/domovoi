//! Typed report lines. Each type renders its canonical value and parses one back strictly.

use core::net::IpAddr;

use super::field::{EncodeError, decode_field, encode_field, not_canonical, split_fields};
use super::scalar::{Cidr, Family, check_ifname, parse_ip, parse_port, parse_u32};
use super::selector::Selector;

fn gateway_text(gateway: Option<IpAddr>) -> String {
    gateway.map_or_else(|| "-".to_owned(), |ip| ip.to_string())
}

fn parse_gateway(text: &str) -> Result<Option<IpAddr>, EncodeError> {
    if text == "-" {
        Ok(None)
    } else {
        parse_ip(text).map(Some)
    }
}

fn parse_ifname(text: &str) -> Result<Vec<u8>, EncodeError> {
    let bytes = decode_field(text)?;
    check_ifname(&bytes)?;
    Ok(bytes)
}

/// `link_peer=<ip>|<port>`: an address the agent's link talks to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPeer {
    /// The peer address.
    pub ip: IpAddr,
    /// The peer port.
    pub port: u16,
}

impl LinkPeer {
    /// The canonical line value.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}|{}", self.ip, self.port)
    }

    /// Parses a line value strictly.
    ///
    /// # Errors
    ///
    /// Any grammar violation.
    pub fn parse(value: &str) -> Result<Self, EncodeError> {
        let fields = split_fields(value, 2)?;
        Ok(Self {
            ip: parse_ip(fields[0])?,
            port: parse_port(fields[1])?,
        })
    }
}

/// `local_addr=<ifaddr>|<ifname>`: an address configured on an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAddr {
    /// The interface address, host bits kept.
    pub addr: Cidr,
    /// The interface name as raw bytes.
    pub ifname: Vec<u8>,
}

impl LocalAddr {
    /// The canonical line value.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}|{}", self.addr.render(), encode_field(&self.ifname))
    }

    /// Parses a line value strictly.
    ///
    /// # Errors
    ///
    /// Any grammar violation.
    pub fn parse(value: &str) -> Result<Self, EncodeError> {
        let fields = split_fields(value, 2)?;
        Ok(Self {
            addr: Cidr::parse_ifaddr(fields[0])?,
            ifname: parse_ifname(fields[1])?,
        })
    }
}

/// `route=<family>|<table>|<dst cidr>|<gateway or ->|<ifname>|<metric>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The address family.
    pub family: Family,
    /// The routing table id.
    pub table: u32,
    /// The destination network.
    pub dst: Cidr,
    /// The gateway, if any.
    pub gateway: Option<IpAddr>,
    /// The output interface as raw bytes.
    pub ifname: Vec<u8>,
    /// The route metric.
    pub metric: u32,
}

impl Route {
    /// The canonical line value.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.family.token(),
            self.table,
            self.dst.render(),
            gateway_text(self.gateway),
            encode_field(&self.ifname),
            self.metric
        )
    }

    /// Parses a line value strictly. The addresses must match the family.
    ///
    /// # Errors
    ///
    /// Any grammar violation, or an address of the other family.
    pub fn parse(value: &str) -> Result<Self, EncodeError> {
        let fields = split_fields(value, 6)?;
        let route = Self {
            family: Family::parse(fields[0])?,
            table: parse_u32(fields[1])?,
            dst: Cidr::parse_report(fields[2])?,
            gateway: parse_gateway(fields[3])?,
            ifname: parse_ifname(fields[4])?,
            metric: parse_u32(fields[5])?,
        };
        let mismatch = route.dst.family() != route.family
            || route.gateway.is_some_and(|g| Family::of(g) != route.family);
        if mismatch {
            return Err(not_canonical("route family", value));
        }
        Ok(route)
    }
}

/// `endpoint_route=<endpoint>|<table>|<gateway or ->|<ifname>|<src>`: the route the kernel
/// selects for one management endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointRoute {
    /// The management endpoint that was looked up.
    pub endpoint: IpAddr,
    /// The table the lookup selected.
    pub table: u32,
    /// The gateway, if any.
    pub gateway: Option<IpAddr>,
    /// The output interface as raw bytes.
    pub ifname: Vec<u8>,
    /// The source address the kernel chose.
    pub src: IpAddr,
}

impl EndpointRoute {
    /// The canonical line value.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.endpoint,
            self.table,
            gateway_text(self.gateway),
            encode_field(&self.ifname),
            self.src
        )
    }

    /// Parses a line value strictly.
    ///
    /// # Errors
    ///
    /// Any grammar violation.
    pub fn parse(value: &str) -> Result<Self, EncodeError> {
        let fields = split_fields(value, 5)?;
        Ok(Self {
            endpoint: parse_ip(fields[0])?,
            table: parse_u32(fields[1])?,
            gateway: parse_gateway(fields[2])?,
            ifname: parse_ifname(fields[3])?,
            src: parse_ip(fields[4])?,
        })
    }
}

/// `rule=<family>|<priority>|<selector as one field>|<table>`: a policy rule that looks up a
/// table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The address family.
    pub family: Family,
    /// The rule priority.
    pub priority: u32,
    /// The selector.
    pub selector: Selector,
    /// The table the rule looks up.
    pub table: u32,
}

impl Rule {
    /// The canonical line value: the whole selector is encoded once as one field.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.family.token(),
            self.priority,
            encode_field(self.selector.render().as_bytes()),
            self.table
        )
    }

    /// Parses a line value strictly.
    ///
    /// # Errors
    ///
    /// Any grammar violation.
    pub fn parse(value: &str) -> Result<Self, EncodeError> {
        let fields = split_fields(value, 4)?;
        let selector = String::from_utf8(decode_field(fields[2])?)
            .map_err(|_| not_canonical("selector utf-8", fields[2]))?;
        Ok(Self {
            family: Family::parse(fields[0])?,
            priority: parse_u32(fields[1])?,
            selector: Selector::parse(&selector)?,
            table: parse_u32(fields[3])?,
        })
    }
}
