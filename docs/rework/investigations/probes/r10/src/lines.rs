//! Typed report lines (04b section 2). Each type renders its canonical value and parses one back
//! strictly: a parsed line is accepted only when re-rendering gives the identical text.

use std::net::IpAddr;

use crate::encoding::{
    Cidr, EncodingError, Family, check_ifname, decode_field, encode_field, parse_cidr, parse_ifaddr,
    parse_ip, parse_port, parse_u32, parse_uint, split_fields,
};

fn nc(kind: &'static str, value: &str) -> EncodingError {
    EncodingError::NotCanonical {
        kind,
        value: value.to_owned(),
    }
}

fn gw_render(g: Option<IpAddr>) -> String {
    g.map_or_else(|| "-".to_owned(), |ip| ip.to_string())
}

fn gw_parse(s: &str) -> Result<Option<IpAddr>, EncodingError> {
    if s == "-" { Ok(None) } else { parse_ip(s).map(Some) }
}

fn ifname_parse(s: &str) -> Result<Vec<u8>, EncodingError> {
    let b = decode_field(s)?;
    check_ifname(&b)?;
    Ok(b)
}

/// `link_peer=<ip>|<port>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPeer {
    /// Peer address.
    pub ip: IpAddr,
    /// Peer port.
    pub port: u16,
}
impl LinkPeer {
    /// Canonical value.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}|{}", self.ip, self.port)
    }
    /// Strict parse.
    ///
    /// # Errors
    /// Any grammar violation.
    pub fn parse(v: &str) -> Result<Self, EncodingError> {
        let f = split_fields(v, 2)?;
        Ok(Self {
            ip: parse_ip(f[0])?,
            port: parse_port(f[1])?,
        })
    }
}

/// `local_addr=<ifaddr>|<ifname>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalAddr {
    /// Interface address, host bits kept.
    pub addr: Cidr,
    /// Interface name (raw bytes).
    pub ifname: Vec<u8>,
}
impl LocalAddr {
    /// Canonical value.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}|{}", self.addr.render(), encode_field(&self.ifname))
    }
    /// Strict parse.
    ///
    /// # Errors
    /// Any grammar violation.
    pub fn parse(v: &str) -> Result<Self, EncodingError> {
        let f = split_fields(v, 2)?;
        Ok(Self {
            addr: parse_ifaddr(f[0])?,
            ifname: ifname_parse(f[1])?,
        })
    }
}

/// `route=<family>|<table>|<dst cidr>|<gw or ->|<ifname>|<metric>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Family.
    pub family: Family,
    /// Table id.
    pub table: u32,
    /// Destination network.
    pub dst: Cidr,
    /// Gateway, if any.
    pub gateway: Option<IpAddr>,
    /// Output interface.
    pub ifname: Vec<u8>,
    /// Metric.
    pub metric: u32,
}
impl Route {
    /// Canonical value.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.family.token(),
            self.table,
            self.dst.render(),
            gw_render(self.gateway),
            encode_field(&self.ifname),
            self.metric
        )
    }
    /// Strict parse. Family consistency with the addresses is enforced (the contract is silent).
    ///
    /// # Errors
    /// Any grammar violation.
    pub fn parse(v: &str) -> Result<Self, EncodingError> {
        let f = split_fields(v, 6)?;
        let r = Self {
            family: Family::parse(f[0])?,
            table: parse_u32(f[1])?,
            dst: parse_cidr(f[2])?,
            gateway: gw_parse(f[3])?,
            ifname: ifname_parse(f[4])?,
            metric: parse_u32(f[5])?,
        };
        if r.dst.family() != r.family || r.gateway.is_some_and(|g| Family::of(g) != r.family) {
            return Err(nc("route family", v));
        }
        Ok(r)
    }
}

/// `endpoint_route=<endpoint>|<table>|<gw or ->|<ifname>|<src>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointRoute {
    /// Management endpoint looked up.
    pub endpoint: IpAddr,
    /// Table the lookup selected.
    pub table: u32,
    /// Gateway, if any.
    pub gateway: Option<IpAddr>,
    /// Output interface.
    pub ifname: Vec<u8>,
    /// Source address the kernel chose.
    pub src: IpAddr,
}
impl EndpointRoute {
    /// Canonical value.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.endpoint,
            self.table,
            gw_render(self.gateway),
            encode_field(&self.ifname),
            self.src
        )
    }
    /// Strict parse.
    ///
    /// # Errors
    /// Any grammar violation.
    pub fn parse(v: &str) -> Result<Self, EncodingError> {
        let f = split_fields(v, 5)?;
        Ok(Self {
            endpoint: parse_ip(f[0])?,
            table: parse_u32(f[1])?,
            gateway: gw_parse(f[2])?,
            ifname: ifname_parse(f[3])?,
            src: parse_ip(f[4])?,
        })
    }
}

/// The selector of a policy rule: `all`, or the items present in the fixed order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selector {
    /// `from=<cidr>`
    pub from: Option<Cidr>,
    /// `to=<cidr>`
    pub to: Option<Cidr>,
    /// `iif=<ifname>` (not escaped inside the selector; see the report)
    pub iif: Option<String>,
    /// `oif=<ifname>`
    pub oif: Option<String>,
    /// `fwmark=0x<hex>/0x<hex>`
    pub fwmark: Option<(u32, u32)>,
    /// `ipproto=<n>`
    pub ipproto: Option<u32>,
    /// `sport=<a>-<b>`
    pub sport: Option<(u32, u32)>,
    /// `dport=<a>-<b>`
    pub dport: Option<(u32, u32)>,
    /// `uidrange=<a>-<b>`
    pub uidrange: Option<(u32, u32)>,
    /// `not`
    pub not: bool,
}

impl Selector {
    /// The unencoded selector text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut items: Vec<String> = Vec::new();
        if let Some(c) = &self.from {
            items.push(format!("from={}", c.render()));
        }
        if let Some(c) = &self.to {
            items.push(format!("to={}", c.render()));
        }
        if let Some(n) = &self.iif {
            items.push(format!("iif={n}"));
        }
        if let Some(n) = &self.oif {
            items.push(format!("oif={n}"));
        }
        if let Some((v, m)) = self.fwmark {
            items.push(format!("fwmark=0x{v:x}/0x{m:x}"));
        }
        if let Some(n) = self.ipproto {
            items.push(format!("ipproto={n}"));
        }
        for (k, r) in [("sport", self.sport), ("dport", self.dport), ("uidrange", self.uidrange)] {
            if let Some((a, b)) = r {
                items.push(format!("{k}={a}-{b}"));
            }
        }
        if self.not {
            items.push("not".to_owned());
        }
        if items.is_empty() { "all".to_owned() } else { items.join(";") }
    }

    /// Strict parse of the decoded selector text; accepted only if it re-renders identically.
    ///
    /// # Errors
    /// `NotCanonical` for any deviation, including order and spelling.
    pub fn parse(s: &str) -> Result<Self, EncodingError> {
        let bad = || nc("selector", s);
        if s == "all" {
            return Ok(Self::default());
        }
        let mut sel = Self::default();
        for item in s.split(';') {
            let (k, v) = item.split_once('=').map_or((item, None), |(k, v)| (k, Some(v)));
            let range = |v: Option<&str>| -> Result<(u32, u32), EncodingError> {
                let (a, b) = v.and_then(|v| v.split_once('-')).ok_or_else(bad)?;
                Ok((parse_u32(a)?, parse_u32(b)?))
            };
            match (k, v) {
                ("from", Some(v)) => sel.from = Some(parse_cidr(v)?),
                ("to", Some(v)) => sel.to = Some(parse_cidr(v)?),
                ("iif", Some(v)) if !v.is_empty() => sel.iif = Some(v.to_owned()),
                ("oif", Some(v)) if !v.is_empty() => sel.oif = Some(v.to_owned()),
                ("fwmark", Some(v)) => {
                    let (a, b) = v.split_once('/').ok_or_else(bad)?;
                    let hex = |h: &str| -> Result<u32, EncodingError> {
                        h.strip_prefix("0x")
                            .and_then(|d| u32::from_str_radix(d, 16).ok())
                            .ok_or_else(bad)
                    };
                    sel.fwmark = Some((hex(a)?, hex(b)?));
                }
                ("ipproto", Some(v)) => sel.ipproto = Some(parse_u32(v)?),
                ("sport", v) => sel.sport = Some(range(v)?),
                ("dport", v) => sel.dport = Some(range(v)?),
                ("uidrange", v) => sel.uidrange = Some(range(v)?),
                ("not", None) => sel.not = true,
                _ => return Err(bad()),
            }
        }
        if sel.render() != s {
            return Err(bad());
        }
        Ok(sel)
    }
}

/// `rule=<family>|<priority>|<selector, percent-encoded as one field>|<table>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Family.
    pub family: Family,
    /// Rule priority.
    pub priority: u32,
    /// Selector.
    pub selector: Selector,
    /// Table the rule looks up.
    pub table: u32,
}
impl Rule {
    /// Canonical value.
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
    /// Strict parse.
    ///
    /// # Errors
    /// Any grammar violation.
    pub fn parse(v: &str) -> Result<Self, EncodingError> {
        let f = split_fields(v, 4)?;
        let sel_bytes = decode_field(f[2])?;
        let sel = String::from_utf8(sel_bytes).map_err(|_| nc("selector utf-8", f[2]))?;
        Ok(Self {
            family: Family::parse(f[0])?,
            priority: parse_u32(f[1])?,
            selector: Selector::parse(&sel)?,
            table: parse_u32(f[3])?,
        })
    }
}

/// Known systemd unit-type suffixes. The contract does not list them; this is the harness's reading.
pub const UNIT_SUFFIXES: &[&str] = &[
    "service", "socket", "target", "mount", "automount", "swap", "timer", "path", "slice", "scope",
    "device",
];

/// Canonical unit name: append `.service` when no known type suffix is present. Case-sensitive.
///
/// # Errors
/// `NotCanonical` for an empty name, over 255 bytes, or characters outside `[A-Za-z0-9:_.@-]`.
pub fn canonical_unit(name: &str) -> Result<String, EncodingError> {
    let ok = !name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'@' | b'-'));
    if !ok {
        return Err(nc("unit name", name));
    }
    let typed = name
        .rsplit_once('.')
        .is_some_and(|(stem, suf)| !stem.is_empty() && UNIT_SUFFIXES.contains(&suf));
    Ok(if typed { name.to_owned() } else { format!("{name}.service") })
}

/// Docker container name grammar `[a-zA-Z0-9][a-zA-Z0-9_.-]*`.
///
/// # Errors
/// `NotCanonical` when the name does not match.
pub fn check_container(name: &str) -> Result<(), EncodingError> {
    let mut it = name.bytes();
    let first = it.next().is_some_and(|b| b.is_ascii_alphanumeric());
    if !first || !it.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) {
        return Err(nc("container name", name));
    }
    Ok(())
}

/// Parse a decimal `u64` field that must be canonical.
///
/// # Errors
/// `NotCanonical`.
pub fn uint(s: &str) -> Result<u64, EncodingError> {
    parse_uint(s)
}
