//! Typed scalars of the report grammar: canonical integers, addresses, prefixes and families.
//!
//! Every parser here accepts a text only when rendering the parsed value gives that exact
//! text back.

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::field::{EncodeError, not_canonical};

/// Parses an unsigned decimal: `0` or a digit string without a leading zero, no sign, fitting
/// `u64`.
///
/// # Errors
///
/// [`EncodeError::NotCanonical`] for anything else.
pub fn parse_uint(text: &str) -> Result<u64, EncodeError> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if !canonical {
        return Err(not_canonical("integer", text));
    }
    text.parse::<u64>()
        .map_err(|_| not_canonical("integer", text))
}

/// Parses a canonical `u32`.
///
/// # Errors
///
/// [`EncodeError::NotCanonical`] when not canonical or above `u32::MAX`.
pub fn parse_u32(text: &str) -> Result<u32, EncodeError> {
    u32::try_from(parse_uint(text)?).map_err(|_| not_canonical("u32", text))
}

/// Parses a canonical TCP or UDP port, 1 to 65535.
///
/// # Errors
///
/// [`EncodeError::NotCanonical`] when out of range or not canonical.
pub fn parse_port(text: &str) -> Result<u16, EncodeError> {
    match u16::try_from(parse_uint(text)?) {
        Ok(port) if port != 0 => Ok(port),
        _ => Err(not_canonical("port", text)),
    }
}

/// Parses canonical IP text: dotted decimal for IPv4, the compressed lowercase form for IPv6.
///
/// # Errors
///
/// [`EncodeError::NotCanonical`] for uppercase or non-compressed IPv6, a leading-zero IPv4
/// octet, or any text that does not render back to itself.
pub fn parse_ip(text: &str) -> Result<IpAddr, EncodeError> {
    let ip: IpAddr = text.parse().map_err(|_| not_canonical("ip", text))?;
    if ip.to_string() == text {
        Ok(ip)
    } else {
        Err(not_canonical("ip", text))
    }
}

/// The address family of a `family` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// `inet`
    Inet,
    /// `inet6`
    Inet6,
}

impl Family {
    /// The wire token.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Inet => "inet",
            Self::Inet6 => "inet6",
        }
    }

    /// The family of an address.
    #[must_use]
    pub const fn of(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(_) => Self::Inet,
            IpAddr::V6(_) => Self::Inet6,
        }
    }

    /// Parses the wire token.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NotCanonical`] for anything but `inet` or `inet6`.
    pub fn parse(text: &str) -> Result<Self, EncodeError> {
        match text {
            "inet" => Ok(Self::Inet),
            "inet6" => Ok(Self::Inet6),
            _ => Err(not_canonical("family", text)),
        }
    }
}

/// Checks an interface name given as decoded bytes: one to fifteen octets. The kernel's own
/// character rules are not enforced on the wire.
///
/// # Errors
///
/// [`EncodeError::NotCanonical`] when empty or longer than fifteen bytes.
pub fn check_ifname(bytes: &[u8]) -> Result<(), EncodeError> {
    if bytes.is_empty() || bytes.len() > 15 {
        return Err(not_canonical("ifname", &String::from_utf8_lossy(bytes)));
    }
    Ok(())
}

/// A network prefix: an address and a length. Host bits are kept as given; [`Cidr::network`]
/// zeroes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cidr {
    /// The address as given.
    pub addr: IpAddr,
    /// The prefix length.
    pub prefix: u8,
}

const fn width(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

fn bits(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u128::from(u32::from(v4)),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

fn from_bits(template: IpAddr, value: u128) -> IpAddr {
    match template {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::from(u32::try_from(value).unwrap_or(0))),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::from(value)),
    }
}

fn split_prefix<'a>(text: &'a str, kind: &'static str) -> Result<(&'a str, u8), EncodeError> {
    let (addr, prefix) = text
        .split_once('/')
        .ok_or_else(|| not_canonical(kind, text))?;
    let prefix = parse_uint(prefix)
        .ok()
        .and_then(|p| u8::try_from(p).ok())
        .ok_or_else(|| not_canonical(kind, text))?;
    Ok((addr, prefix))
}

impl Cidr {
    /// Builds a prefix, checking the length against the address family.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NotCanonical`] when the length exceeds the family's width.
    pub fn new(addr: IpAddr, prefix: u8) -> Result<Self, EncodeError> {
        if prefix > width(addr) {
            return Err(not_canonical("prefix", &prefix.to_string()));
        }
        Ok(Self { addr, prefix })
    }

    /// The host prefix (`/32` or `/128`) of one address.
    #[must_use]
    pub const fn host(addr: IpAddr) -> Self {
        Self {
            addr,
            prefix: width(addr),
        }
    }

    fn mask(&self) -> u128 {
        let total = width(self.addr);
        let all = if total == 32 {
            u128::from(u32::MAX)
        } else {
            u128::MAX
        };
        if self.prefix == 0 {
            0
        } else {
            (all << (total - self.prefix)) & all
        }
    }

    /// The same prefix with the host bits zeroed.
    #[must_use]
    pub fn network(&self) -> Self {
        Self {
            addr: from_bits(self.addr, bits(self.addr) & self.mask()),
            prefix: self.prefix,
        }
    }

    /// Whether the host bits are all zero.
    #[must_use]
    pub fn is_network(&self) -> bool {
        self.network().addr == self.addr
    }

    /// The family of the address.
    #[must_use]
    pub const fn family(&self) -> Family {
        Family::of(self.addr)
    }

    /// Whether `self` lies entirely inside `outer`; the families must match.
    #[must_use]
    pub fn subset_of(&self, outer: &Self) -> bool {
        self.family() == outer.family()
            && self.prefix >= outer.prefix
            && (bits(self.addr) & outer.mask()) == (bits(outer.addr) & outer.mask())
    }

    /// Whether the two prefixes share any address.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.subset_of(other) || other.subset_of(self)
    }

    /// The `address/prefix` text.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}/{}", self.addr, self.prefix)
    }

    /// Parses a report `cidr`: canonical address with the host bits zero, then `/prefix`.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NotCanonical`] for non-canonical text, set host bits or a too-long
    /// prefix.
    pub fn parse_report(text: &str) -> Result<Self, EncodeError> {
        let (addr, prefix) = split_prefix(text, "cidr")?;
        let cidr = Self::new(
            parse_ip(addr).map_err(|_| not_canonical("cidr", text))?,
            prefix,
        )
        .map_err(|_| not_canonical("cidr", text))?;
        if cidr.is_network() {
            Ok(cidr)
        } else {
            Err(not_canonical("cidr", text))
        }
    }

    /// Parses a report `ifaddr`: canonical address with its host bits kept, then `/prefix`.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NotCanonical`] for non-canonical text or a too-long prefix.
    pub fn parse_ifaddr(text: &str) -> Result<Self, EncodeError> {
        let (addr, prefix) = split_prefix(text, "ifaddr")?;
        Self::new(
            parse_ip(addr).map_err(|_| not_canonical("ifaddr", text))?,
            prefix,
        )
        .map_err(|_| not_canonical("ifaddr", text))
    }

    /// Parses a configured CIDR leniently and normalises it: any address the standard parser
    /// reads, a digits-only prefix, host bits zeroed.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NotCanonical`] when the text is not `address/prefix`, including a
    /// missing prefix.
    pub fn parse_config(text: &str) -> Result<Self, EncodeError> {
        let (addr, prefix) = text
            .split_once('/')
            .ok_or_else(|| not_canonical("cidr", text))?;
        if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
            return Err(not_canonical("cidr", text));
        }
        let addr: IpAddr = addr.parse().map_err(|_| not_canonical("cidr", text))?;
        let prefix: u8 = prefix.parse().map_err(|_| not_canonical("cidr", text))?;
        Ok(Self::new(addr, prefix)
            .map_err(|_| not_canonical("cidr", text))?
            .network())
    }
}
