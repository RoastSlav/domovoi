//! Canonical encoding and strict decoding for the report grammar (04b section 1).
//!
//! Every typed value is accepted only when `render(parse(x)) == x`, byte for byte. That single
//! rule gives RFC 5952 IPv6, no leading zeros, zeroed host bits and canonical integers.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Why a field or line failed the strict grammar. The `Display` text is quoted in the report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EncodingError {
    /// A raw byte outside the literal set and not `%`.
    #[error("raw byte 0x{0:02X} outside the literal set")]
    RawByte(u8),
    /// A `%` with fewer than two following bytes.
    #[error("truncated percent escape")]
    TruncatedEscape,
    /// A `%` followed by something that is not hex.
    #[error("percent escape without two hex digits: %{0}")]
    BadHex(String),
    /// A `%` escape that uses lowercase hex.
    #[error("lowercase hex in percent escape: %{0}")]
    LowercaseHex(String),
    /// A `%` escape of a byte that must appear literally.
    #[error("escape %{0} encodes a byte of the literal set")]
    EncodedLiteral(String),
    /// A line value with the wrong number of `|` separated fields.
    #[error("expected {expected} fields, found {found}")]
    FieldCount {
        /// Fields the key defines.
        expected: usize,
        /// Fields present.
        found: usize,
    },
    /// A typed value that is not canonical.
    #[error("not a canonical {kind}: {value:?}")]
    NotCanonical {
        /// Which type was expected.
        kind: &'static str,
        /// The offending text.
        value: String,
    },
}

/// True for the bytes that appear literally in a field.
#[must_use]
pub fn is_literal(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'@' | b'-')
}

/// Strict percent-encoding: literal set as is, everything else `%XX` uppercase.
#[must_use]
pub fn encode_field(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if is_literal(b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Strict decoding of one field. Rejects lowercase hex, encoded literals, truncated escapes and
/// any raw byte outside the literal set plus `%`.
///
/// # Errors
/// Returns the first violation found, scanning left to right.
pub fn decode_field(s: &str) -> Result<Vec<u8>, EncodingError> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' {
            if i + 3 > b.len() {
                return Err(EncodingError::TruncatedEscape);
            }
            let digits = &b[i + 1..i + 3];
            let pair = String::from_utf8_lossy(digits).into_owned();
            if !digits.iter().all(u8::is_ascii_hexdigit) {
                return Err(EncodingError::BadHex(pair));
            }
            if digits.iter().any(u8::is_ascii_lowercase) {
                return Err(EncodingError::LowercaseHex(pair));
            }
            let v = u8::from_str_radix(&pair, 16).map_err(|_| EncodingError::BadHex(pair.clone()))?;
            if is_literal(v) {
                return Err(EncodingError::EncodedLiteral(pair));
            }
            out.push(v);
            i += 3;
        } else if is_literal(c) {
            out.push(c);
            i += 1;
        } else {
            return Err(EncodingError::RawByte(c));
        }
    }
    Ok(out)
}

fn nc(kind: &'static str, value: &str) -> EncodingError {
    EncodingError::NotCanonical {
        kind,
        value: value.to_owned(),
    }
}

/// Unsigned decimal: digits only, no sign, no leading zeros (`0` allowed), fits `u64`.
///
/// # Errors
/// `NotCanonical` for anything else.
pub fn parse_uint(s: &str) -> Result<u64, EncodingError> {
    let ok = !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'));
    if !ok {
        return Err(nc("integer", s));
    }
    s.parse::<u64>().map_err(|_| nc("integer", s))
}

/// Canonical `u32`.
///
/// # Errors
/// `NotCanonical` when not canonical or above `u32::MAX`.
pub fn parse_u32(s: &str) -> Result<u32, EncodingError> {
    u32::try_from(parse_uint(s)?).map_err(|_| nc("u32", s))
}

/// Canonical TCP/UDP port, 1 to 65535 (the contract does not say whether 0 is allowed; this
/// harness refuses it).
///
/// # Errors
/// `NotCanonical` when out of range or not canonical.
pub fn parse_port(s: &str) -> Result<u16, EncodingError> {
    let v = u16::try_from(parse_uint(s)?).map_err(|_| nc("port", s))?;
    if v == 0 {
        return Err(nc("port", s));
    }
    Ok(v)
}

/// Canonical IP address text (IPv4 dotted decimal, IPv6 RFC 5952 lowercase as std prints it).
///
/// # Errors
/// `NotCanonical` when the text does not round-trip through the standard formatter.
pub fn parse_ip(s: &str) -> Result<IpAddr, EncodingError> {
    let ip: IpAddr = s.parse().map_err(|_| nc("ip", s))?;
    if ip.to_string() != s {
        return Err(nc("ip", s));
    }
    Ok(ip)
}

/// Address family of the `family` field.
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
    pub fn token(self) -> &'static str {
        match self {
            Self::Inet => "inet",
            Self::Inet6 => "inet6",
        }
    }
    /// Family of an address.
    #[must_use]
    pub fn of(ip: IpAddr) -> Self {
        if ip.is_ipv4() { Self::Inet } else { Self::Inet6 }
    }
    /// Parse the wire token.
    ///
    /// # Errors
    /// `NotCanonical` for anything but `inet` or `inet6`.
    pub fn parse(s: &str) -> Result<Self, EncodingError> {
        match s {
            "inet" => Ok(Self::Inet),
            "inet6" => Ok(Self::Inet6),
            _ => Err(nc("family", s)),
        }
    }
}

/// A network prefix: address bits plus length. Host bits are kept as given; `network()` zeroes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cidr {
    /// The address as given.
    pub addr: IpAddr,
    /// Prefix length.
    pub prefix: u8,
}

fn bits(ip: IpAddr) -> (u128, u8) {
    match ip {
        IpAddr::V4(v) => (u128::from(u32::from(v)), 32),
        IpAddr::V6(v) => (u128::from(v), 128),
    }
}

impl Cidr {
    /// Build, checking the prefix against the family width.
    ///
    /// # Errors
    /// `NotCanonical` when the prefix is too long for the family.
    pub fn new(addr: IpAddr, prefix: u8) -> Result<Self, EncodingError> {
        if prefix > bits(addr).1 {
            return Err(nc("prefix", &prefix.to_string()));
        }
        Ok(Self { addr, prefix })
    }

    fn mask(&self) -> u128 {
        let (_, w) = bits(self.addr);
        if self.prefix == 0 {
            0
        } else {
            let all = if w == 32 { u128::from(u32::MAX) } else { u128::MAX };
            (all << (w - self.prefix)) & all
        }
    }

    /// The same prefix with host bits zeroed.
    #[must_use]
    pub fn network(&self) -> Self {
        let (a, w) = bits(self.addr);
        let n = a & self.mask();
        let addr = if w == 32 {
            IpAddr::V4(Ipv4Addr::from(u32::try_from(n).unwrap_or(0)))
        } else {
            IpAddr::V6(Ipv6Addr::from(n))
        };
        Self {
            addr,
            prefix: self.prefix,
        }
    }

    /// True when host bits are all zero.
    #[must_use]
    pub fn is_network(&self) -> bool {
        self.network().addr == self.addr
    }

    /// Family of the prefix.
    #[must_use]
    pub fn family(&self) -> Family {
        Family::of(self.addr)
    }

    /// True when `self` is entirely inside `outer` (same family required).
    #[must_use]
    pub fn subset_of(&self, outer: &Self) -> bool {
        self.family() == outer.family()
            && self.prefix >= outer.prefix
            && (bits(self.addr).0 & outer.mask()) == (bits(outer.addr).0 & outer.mask())
    }

    /// True when the two prefixes share any address.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.subset_of(other) || other.subset_of(self)
    }

    /// `addr/prefix` text.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}/{}", self.addr, self.prefix)
    }

    /// A host prefix (/32 or /128) for one address.
    #[must_use]
    pub fn host(ip: IpAddr) -> Self {
        Self {
            addr: ip,
            prefix: bits(ip).1,
        }
    }
}

fn split_prefix<'a>(s: &'a str, kind: &'static str) -> Result<(&'a str, u8), EncodingError> {
    let (a, p) = s.split_once('/').ok_or_else(|| nc(kind, s))?;
    let p8 = u8::try_from(parse_uint(p).map_err(|_| nc(kind, s))?).map_err(|_| nc(kind, s))?;
    Ok((a, p8))
}

/// Canonical `cidr` field: network address with host bits zeroed, then `/prefix`.
///
/// # Errors
/// `NotCanonical` for non-canonical text, set host bits or a too-long prefix.
pub fn parse_cidr(s: &str) -> Result<Cidr, EncodingError> {
    let (a, p) = split_prefix(s, "cidr")?;
    let c = Cidr::new(parse_ip(a).map_err(|_| nc("cidr", s))?, p).map_err(|_| nc("cidr", s))?;
    if !c.is_network() {
        return Err(nc("cidr", s));
    }
    Ok(c)
}

/// Canonical `ifaddr` field: interface address keeping host bits, then `/prefix`.
///
/// # Errors
/// `NotCanonical` for non-canonical text or a too-long prefix.
pub fn parse_ifaddr(s: &str) -> Result<Cidr, EncodingError> {
    let (a, p) = split_prefix(s, "ifaddr")?;
    Cidr::new(parse_ip(a).map_err(|_| nc("ifaddr", s))?, p).map_err(|_| nc("ifaddr", s))
}

/// Lenient configuration-side CIDR: any std-parsable address, digits-only prefix. The result is
/// normalised by [`Cidr::network`] before it is emitted.
///
/// # Errors
/// `NotCanonical` when the text is not `address/prefix`.
pub fn parse_cidr_config(s: &str) -> Result<Cidr, EncodingError> {
    let (a, p) = s.split_once('/').ok_or_else(|| nc("cidr", s))?;
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
        return Err(nc("cidr", s));
    }
    let addr: IpAddr = a.parse().map_err(|_| nc("cidr", s))?;
    let p8: u8 = p.parse().map_err(|_| nc("cidr", s))?;
    Ok(Cidr::new(addr, p8).map_err(|_| nc("cidr", s))?.network())
}

/// Interface name bytes: non-empty and at most 15 bytes (IFNAMSIZ - 1). The contract states no
/// grammar; see the report.
///
/// # Errors
/// `NotCanonical` when empty or too long.
pub fn check_ifname(b: &[u8]) -> Result<(), EncodingError> {
    if b.is_empty() || b.len() > 15 {
        return Err(nc("ifname", &String::from_utf8_lossy(b)));
    }
    Ok(())
}

/// Split a value into exactly `n` `|` separated fields.
///
/// # Errors
/// `FieldCount` when the number differs.
pub fn split_fields(value: &str, n: usize) -> Result<Vec<&str>, EncodingError> {
    let f: Vec<&str> = value.split('|').collect();
    if f.len() != n {
        return Err(EncodingError::FieldCount {
            expected: n,
            found: f.len(),
        });
    }
    Ok(f)
}
