//! A canonical network: one address or a prefix of one family, parsed strictly.

use serde::{Deserialize, Serialize};
use std::fmt;

use super::ip::{format_v4, format_v6, parse_v4, parse_v6};

/// The address family of a [`Net`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// IPv4.
    V4,
    /// IPv6.
    V6,
}

impl Family {
    /// The longest prefix of the family: 32 or 128.
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::V4 => 32,
            Self::V6 => 128,
        }
    }
}

/// Why text is not a usable [`Net`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    /// Not an IPv4 or IPv6 address with an optional prefix length. Hostnames land here.
    #[error("not an IPv4 or IPv6 address or prefix")]
    Invalid,
    /// An IPv4-mapped or IPv4-compatible IPv6 range, which is ambiguous with IPv4.
    #[error("IPv4-mapped IPv6 ranges are not accepted")]
    MappedForm,
    /// The address has bits set below the prefix length.
    #[error("the address has host bits set below the prefix length")]
    HostBits,
    /// The prefix length is not a number in range for the family.
    #[error("the prefix length is out of range")]
    PrefixLength,
}

/// A canonical network: one family, `lo..=hi` as `u128` and the prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Net {
    family: Family,
    lo: u128,
    hi: u128,
    prefix: u8,
}

/// `::ffff:0:0`, the base of the IPv4-mapped range.
const MAPPED_BASE: u128 = 0xffff_0000_0000;
/// The last address of `::ffff:0:0/96`.
const MAPPED_END: u128 = MAPPED_BASE + 0xffff_ffff;
/// The last address of `::/96`.
const LOW96_END: u128 = 0xffff_ffff;

impl Net {
    /// Strict parse: dotted IPv4 without leading zeros or IPv6 text, an optional `/len`, no
    /// hostnames and zero host bits.
    ///
    /// # Errors
    /// [`NetError`] naming the first problem; `MappedForm` for `::ffff:0:0/96` and `::/96`
    /// ranges.
    pub fn parse(text: &str) -> Result<Self, NetError> {
        let net = Self::parse_any(text)?;
        if net.family == Family::V6 {
            let in_low96 = net.lo <= LOW96_END;
            let touches_mapped = net.lo <= MAPPED_END && net.hi >= MAPPED_BASE;
            if in_low96 || touches_mapped {
                return Err(NetError::MappedForm);
            }
        }
        Ok(net)
    }

    /// Parses an exclusion member: like [`Net::parse`], but a mapped member is turned into
    /// the IPv4 network it stands for so it is compared in the right family.
    ///
    /// # Errors
    /// [`NetError`] when the text is not a network at all.
    pub fn parse_exclusion(text: &str) -> Result<Self, NetError> {
        Ok(Self::parse_any(text)?.canonicalise_mapped())
    }

    /// The same network with an IPv4-mapped IPv6 range written as IPv4.
    #[must_use]
    pub fn canonicalise_mapped(self) -> Self {
        if self.family == Family::V6
            && self.prefix >= 96
            && self.lo >= MAPPED_BASE
            && self.hi <= MAPPED_END
        {
            return Self {
                family: Family::V4,
                lo: self.lo - MAPPED_BASE,
                hi: self.hi - MAPPED_BASE,
                prefix: self.prefix - 96,
            };
        }
        self
    }

    fn parse_any(text: &str) -> Result<Self, NetError> {
        let (addr, len) = match text.split_once('/') {
            Some((addr, len)) => (addr, Some(len)),
            None => (text, None),
        };
        let (family, value) = if let Some(v4) = parse_v4(addr) {
            (Family::V4, u128::from(v4))
        } else if let Some(v6) = parse_v6(addr) {
            (Family::V6, v6)
        } else {
            return Err(NetError::Invalid);
        };
        let bits = family.bits();
        let prefix = match len {
            None => bits,
            Some(len) => parse_prefix(len, bits)?,
        };
        let host_bits = u32::from(bits - prefix);
        let mask = if host_bits >= 128 {
            u128::MAX
        } else {
            (1u128 << host_bits) - 1
        };
        if value & mask != 0 {
            return Err(NetError::HostBits);
        }
        Ok(Self {
            family,
            lo: value,
            hi: value | mask,
            prefix,
        })
    }

    /// Builds a network from its parts for the fixed tables; callers pass valid values.
    pub(crate) const fn raw(family: Family, lo: u128, hi: u128, prefix: u8) -> Self {
        Self {
            family,
            lo,
            hi,
            prefix,
        }
    }

    /// The canonical text: dotted IPv4 or lowercase RFC 5952 IPv6, with `/len` always written.
    #[must_use]
    pub fn canonical(&self) -> String {
        format!("{}/{}", addr_text(self.family, self.lo), self.prefix)
    }

    /// The element text nft takes: the address alone for a single host, else `addr/len`.
    #[must_use]
    pub fn element_text(&self) -> String {
        if self.is_single_host() {
            addr_text(self.family, self.lo)
        } else {
            self.canonical()
        }
    }

    /// The address family.
    #[must_use]
    pub const fn family(&self) -> Family {
        self.family
    }

    /// The prefix length.
    #[must_use]
    pub const fn prefix_len(&self) -> u8 {
        self.prefix
    }

    /// First address of the range, as a number.
    #[must_use]
    pub const fn lo(&self) -> u128 {
        self.lo
    }

    /// Last address of the range, as a number.
    #[must_use]
    pub const fn hi(&self) -> u128 {
        self.hi
    }

    /// Whether this is one address: a `/32` or a `/128`.
    #[must_use]
    pub const fn is_single_host(&self) -> bool {
        self.prefix == self.family.bits()
    }

    /// Whether the two ranges share an address. One test for both directions; networks of
    /// different families never overlap.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.family == other.family && self.lo <= other.hi && other.lo <= self.hi
    }
}

fn parse_prefix(len: &str, bits: u8) -> Result<u8, NetError> {
    let digits = !len.is_empty() && len.len() <= 3 && len.bytes().all(|b| b.is_ascii_digit());
    if !digits || (len.len() > 1 && len.starts_with('0')) {
        return Err(NetError::PrefixLength);
    }
    let n: u8 = len.parse().map_err(|_| NetError::PrefixLength)?;
    if n > bits {
        return Err(NetError::PrefixLength);
    }
    Ok(n)
}

/// The text of one address of `family`; the value is truncated to 32 bits for IPv4.
#[must_use]
pub fn addr_text(family: Family, value: u128) -> String {
    match family {
        Family::V4 => format_v4(value as u32),
        Family::V6 => format_v6(value),
    }
}

impl fmt::Display for Net {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

impl TryFrom<String> for Net {
    type Error = NetError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Net> for String {
    fn from(value: Net) -> Self {
        value.canonical()
    }
}
