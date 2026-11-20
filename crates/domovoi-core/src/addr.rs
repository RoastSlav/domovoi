//! Address ranges in CIDR form, such as `192.168.1.0/24` or `fd00::/8`, and single addresses.
//!
//! The core crate is pure (R2), so addresses are parsed and compared by hand: IPv4 as four
//! decimal octets, IPv6 as up to eight hexadecimal groups with one `::` run of zeros. A bare
//! address is the range of that one machine, and an IPv4-mapped IPv6 address, such as
//! `::ffff:192.168.1.20`, is read as the IPv4 address it maps, the way the SSH check's
//! network classification reads it.

/// An address as the bits a range compares, and whether it is IPv4 (whose ranges are 32 bits
/// wide, so the address sits in the low 32 bits).
struct Address {
    bits: u128,
    v4: bool,
}

/// The width in bits of an address's family.
fn width(address: &Address) -> u32 {
    if address.v4 { 32 } else { 128 }
}

/// `cidr` is a well-formed address range, such as `192.168.1.0/24` or `fd00::/8`.
pub fn valid_cidr(cidr: &str) -> bool {
    split_cidr(cidr).is_some()
}

/// Whether `ip` is an address inside the range `cidr`. A range of the other family never
/// matches, and neither does text that is not an address, not a range, or a prefix wider
/// than its family.
pub fn in_cidr(ip: &str, cidr: &str) -> bool {
    let Some((range, prefix)) = split_cidr(cidr) else {
        return false;
    };
    let Some(address) = parse_address(ip) else {
        return false;
    };
    if address.v4 != range.v4 {
        return false;
    }
    if prefix == 0 {
        return true;
    }
    // Shift out the bits past the prefix, so only the bits the range fixes are compared.
    // The prefix is not 0 here, so the shift stays within the width.
    let shared = width(&address) - u32::from(prefix);
    address.bits >> shared == range.bits >> shared
}

/// The canonical text of a range: host bits zeroed, then `/prefix`; IPv4 as dotted decimal,
/// IPv6 as lowercase RFC 5952 (the longest run of two or more zero groups compressed). A bare
/// address is the range of that one machine. `None` when the text is not a range.
pub fn canonical_cidr(text: &str) -> Option<String> {
    let (address, prefix) = split_cidr(text)?;
    let bits_wide = width(&address);
    let keep = u32::from(prefix);
    let mask = if keep == 0 {
        0
    } else {
        u128::MAX << (bits_wide - keep)
    };
    let all = if address.v4 { 0xffff_ffff } else { u128::MAX };
    let bits = address.bits & mask & all;
    if address.v4 {
        let octets = [24, 16, 8, 0].map(|shift| (bits >> shift) & 0xff);
        return Some(format!(
            "{}.{}.{}.{}/{prefix}",
            octets[0], octets[1], octets[2], octets[3]
        ));
    }
    let groups: Vec<u16> = (0..8)
        .map(|i| u16::try_from((bits >> (112 - 16 * i)) & 0xffff).unwrap_or(0))
        .collect();
    Some(format!("{}/{prefix}", compress_ipv6(&groups)))
}

/// RFC 5952 text of eight groups.
fn compress_ipv6(groups: &[u16]) -> String {
    let (mut best_start, mut best_len) = (0usize, 0usize);
    let mut i = 0;
    while i < groups.len() {
        if groups[i] == 0 {
            let start = i;
            while i < groups.len() && groups[i] == 0 {
                i += 1;
            }
            if i - start > best_len {
                best_start = start;
                best_len = i - start;
            }
        } else {
            i += 1;
        }
    }
    let join = |part: &[u16]| {
        part.iter()
            .map(|g| format!("{g:x}"))
            .collect::<Vec<_>>()
            .join(":")
    };
    if best_len < 2 {
        return join(groups);
    }
    format!(
        "{}::{}",
        join(&groups[..best_start]),
        join(&groups[best_start + best_len..])
    )
}

/// The address and prefix of a `cidr`; `None` when the text is not an address range.
/// A bare address with no `/` is the range of that one machine: `192.168.1.20` is
/// `192.168.1.20/32`, and `fd00::20` is `fd00::20/128`.
fn split_cidr(cidr: &str) -> Option<(Address, u8)> {
    let text = cidr.trim();
    match text.split_once('/') {
        Some((address, prefix)) => {
            let address = parse_address(address)?;
            let prefix = prefix.parse::<u8>().ok()?;
            if u32::from(prefix) > width(&address) {
                return None;
            }
            Some((address, prefix))
        }
        None => {
            let address = parse_address(text)?;
            let full = u8::try_from(width(&address)).ok()?;
            Some((address, full))
        }
    }
}

/// An address from text: IPv4 when it has dots, IPv6 when it has `:`. An IPv4-mapped IPv6
/// address, such as `::ffff:192.168.1.20`, is read as the IPv4 address it maps, the way the
/// SSH check's network classification reads it.
fn parse_address(text: &str) -> Option<Address> {
    let text = text.trim();
    if text.contains(':') {
        let bits = bits_of(parse_ipv6_groups(text)?);
        // The `::ffff:` block: the top 80 bits are zero and the next 16 are `ffff`, so the
        // address is the IPv4 one in the low 32 bits.
        let mapped = bits >> 32 == 0xffff;
        Some(Address {
            bits: if mapped { bits & 0xffff_ffff } else { bits },
            v4: mapped,
        })
    } else {
        Some(Address {
            bits: u128::from(parse_ipv4_octets(text)?),
            v4: true,
        })
    }
}

/// Four decimal octets of 0-255, with no leading zeros, as a 32-bit address.
fn parse_ipv4_octets(text: &str) -> Option<u32> {
    let words = text.split('.').collect::<Vec<_>>();
    if words.len() != 4 {
        return None;
    }
    let mut value = 0u32;
    for word in words {
        if word.is_empty() || word.len() > 3 || !word.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if word.len() > 1 && word.starts_with('0') {
            return None;
        }
        let octet = word.parse::<u32>().ok()?;
        if octet > 255 {
            return None;
        }
        value = value << 8 | octet;
    }
    Some(value)
}

/// The eight groups of an IPv6 address, with one `::` standing for a run of zeros. The
/// `::` must replace at least one group, so its two sides leave room for one.
fn parse_ipv6_groups(text: &str) -> Option<[u16; 8]> {
    let (head, tail) = match text.split_once("::") {
        Some((head, tail)) => (split_groups(head)?, Some(split_groups(tail)?)),
        None => (split_groups(text)?, None),
    };
    let mut groups = [0u16; 8];
    match tail {
        Some(tail) => {
            if head.len() + tail.len() > 7 {
                return None;
            }
            for (index, group) in head.iter().enumerate() {
                groups[index] = *group;
            }
            for (index, group) in tail.iter().enumerate() {
                groups[8 - tail.len() + index] = *group;
            }
        }
        None => {
            if head.len() != 8 {
                return None;
            }
            for (index, group) in head.iter().enumerate() {
                groups[index] = *group;
            }
        }
    }
    Some(groups)
}

/// The groups of one side of a `::`: 1-4 hex digits each, with the last one allowed to be
/// an IPv4 address, as in `::ffff:192.168.1.20`. Empty text gives no groups.
fn split_groups(text: &str) -> Option<Vec<u16>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    let words = text.split(':').collect::<Vec<_>>();
    let last = words.len() - 1;
    let mut groups = Vec::new();
    for (index, word) in words.iter().enumerate() {
        if word.contains('.') {
            if index != last {
                return None;
            }
            let octets = parse_ipv4_octets(word)?;
            groups.push((octets >> 16) as u16);
            groups.push(octets as u16);
            continue;
        }
        if word.is_empty() || word.len() > 4 || !word.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        groups.push(u16::from_str_radix(word, 16).ok()?);
    }
    Some(groups)
}

/// The groups as 128 bits.
fn bits_of(groups: [u16; 8]) -> u128 {
    let mut bits = 0u128;
    for group in groups {
        bits = bits << 16 | u128::from(group);
    }
    bits
}

#[cfg(test)]
mod tests {
    #[test]
    fn canonical_cidr_zeroes_host_bits_and_compresses_ipv6() {
        use super::canonical_cidr;
        let good = [
            ("192.168.1.7/24", "192.168.1.0/24"),
            ("10.0.0.5", "10.0.0.5/32"),
            ("2001:DB8:0:0:0:0:0:0/32", "2001:db8::/32"),
            ("fd00::1", "fd00::1/128"),
            ("1:0:0:2:0:0:0:3", "1:0:0:2::3/128"),
            ("1:0:3:4:5:6:7:8", "1:0:3:4:5:6:7:8/128"),
            ("::ffff:10.0.0.7", "10.0.0.7/32"),
            ("::/0", "::/0"),
        ];
        for (input, want) in good {
            assert_eq!(canonical_cidr(input).as_deref(), Some(want), "{input}");
        }
        for bad in ["10.0.0.0/33", "::ffff:10.0.0.7/104", "x", ""] {
            assert_eq!(canonical_cidr(bad), None, "{bad}");
        }
    }

    use super::*;

    #[test]
    fn in_network_matches_the_addresses_inside_a_range() {
        let cases = [
            ("192.168.1.20", "192.168.1.0/24", true),
            ("192.168.1.0", "192.168.1.0/24", true),
            ("192.168.1.255", "192.168.1.0/24", true),
            ("192.168.2.20", "192.168.1.0/24", false),
            ("10.0.0.5", "192.168.1.0/24", false),
            ("192.168.1.20", "192.168.1.20/32", true),
            ("192.168.1.21", "192.168.1.20/32", false),
            ("8.8.8.8", "0.0.0.0/0", true),
            ("255.255.255.255", "0.0.0.0/0", true),
            ("fd00::20", "fd00::/8", true),
            ("fd12::34", "fd00::/8", true),
            ("fe12::34", "fd00::/8", false),
            ("fe80::1", "fe80::/10", true),
            ("feff::1", "fe80::/10", false),
            ("fe80:1::", "fe80::/64", false),
            ("fd00::1", "fd00::/128", false),
            ("fd00::", "fd00::/128", true),
            ("::1", "::/0", true),
            ("2001:db8::1", "2001:db8::/32", true),
            ("1:2:3:4:5:6:7:8", "1::/16", true),
            // A range of the other family never matches, and neither does text that is
            // not an address, a range, or a prefix out of range. An IPv4-mapped IPv6
            // address is read as the IPv4 address it maps, so it matches IPv4 ranges and
            // not IPv6 ones.
            ("192.168.1.20", "fd00::/8", false),
            ("fd00::20", "192.168.1.0/24", false),
            ("::ffff:192.168.1.20", "fd00::/8", false),
            ("::ffff:192.168.1.20", "::/0", false),
            ("::ffff:192.168.1.20", "192.168.1.0/24", true),
            ("not-an-address", "192.168.1.0/24", false),
            ("192.168.1.20", "192.168.1.0/33", false),
            ("192.168.1.20", "192.168.1.0/abc", false),
            ("192.168.1.20", "", false),
            ("192.168.1.20", "abc/24", false),
            ("192.168.01.20", "192.168.1.0/24", false),
            ("192.168.1.20", "192.168.01.0/24", false),
            ("192.168.1.256", "192.168.1.0/24", false),
            // A single address is the range of that one machine.
            ("192.168.1.20", "192.168.1.20", true),
            ("192.168.1.21", "192.168.1.20", false),
            ("192.168.1.20", "192.168.1.0", false),
            ("fd00::20", "fd00::20", true),
            ("fd00::21", "fd00::20", false),
            ("::ffff:192.168.1.20", "192.168.1.20", true),
            ("192.168.1.20", "not-an-address", false),
            ("1:2:3:4:5:6:7", "1::/16", false),
            ("1:2:3:4:5:6:7:8:9", "1::/16", false),
            ("1:2:3:4:5:6:7::8:9", "1::/16", false),
            ("12345::1", "1::/16", false),
            ("1.2.3.4:5", "::/0", false),
        ];
        for (ip, cidr, expected) in cases {
            assert_eq!(in_cidr(ip, cidr), expected, "{ip} in {cidr}");
        }
    }

    #[test]
    fn valid_cidr_accepts_only_well_formed_ranges() {
        for ok in [
            "192.168.1.0/24",
            "10.0.0.0/8",
            "0.0.0.0/0",
            "fd00::/8",
            "fe80::1/128",
            "fe80:1::1/64",
            "::/0",
            "::ffff:192.168.1.20/32",
            "192.168.1.20",
            "fd00::20",
            "::ffff:192.168.1.20",
        ] {
            assert!(valid_cidr(ok), "{ok}");
        }
        for bad in [
            "",
            "192.168.1.0/",
            "/24",
            "192.168.1.0/33",
            "fd00::/129",
            "192.168.01.0/24",
            "fe80::1/abc",
            "fe80:1::1/129",
            "not-an-address",
            // An IPv4-mapped IPv6 address is read as the IPv4 address it maps, so its
            // prefix is the IPv4 width, and `/128` is out of range.
            "::ffff:192.168.1.20/128",
        ] {
            assert!(!valid_cidr(bad), "{bad}");
        }
    }
}
