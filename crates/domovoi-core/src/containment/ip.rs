//! Strict IP address text, parsed and written without `std::net` (the crate is pure).

/// Parses dotted IPv4: four decimal parts of at most 255, no leading zeros, no spaces.
pub(super) fn parse_v4(text: &str) -> Option<u32> {
    let mut value: u32 = 0;
    let mut parts = 0;
    for part in text.split('.') {
        let digits =
            !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit());
        if !digits || (part.len() > 1 && part.starts_with('0')) {
            return None;
        }
        let n: u32 = part.parse().ok()?;
        if n > 255 {
            return None;
        }
        value = (value << 8) | n;
        parts += 1;
    }
    (parts == 4).then_some(value)
}

fn parse_groups(text: &str) -> Option<Vec<u16>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let parts: Vec<&str> = text.split(':').collect();
    for (index, part) in parts.iter().enumerate() {
        if index + 1 == parts.len() && part.contains('.') {
            let v4 = parse_v4(part)?;
            out.push((v4 >> 16) as u16);
            out.push((v4 & 0xffff) as u16);
            continue;
        }
        let ok = !part.is_empty() && part.len() <= 4 && part.bytes().all(|b| b.is_ascii_hexdigit());
        if !ok {
            return None;
        }
        out.push(u16::from_str_radix(part, 16).ok()?);
    }
    Some(out)
}

/// Parses IPv6 text (RFC 4291): hex groups, one optional `::`, an optional trailing dotted
/// IPv4. Input may use upper case.
pub(super) fn parse_v6(text: &str) -> Option<u128> {
    let groups: Vec<u16> = match text.split_once("::") {
        Some((head, tail)) => {
            if tail.contains("::") {
                return None;
            }
            let head = parse_groups(head)?;
            let tail = parse_groups(tail)?;
            let used = head.len() + tail.len();
            if used > 7 {
                return None;
            }
            let mut all = head;
            all.extend(std::iter::repeat_n(0, 8 - used));
            all.extend(tail);
            all
        }
        None => {
            let all = parse_groups(text)?;
            if all.len() != 8 || text.is_empty() {
                return None;
            }
            all
        }
    };
    Some(
        groups
            .iter()
            .fold(0u128, |acc, g| (acc << 16) | u128::from(*g)),
    )
}

/// Writes dotted IPv4.
pub(super) fn format_v4(value: u32) -> String {
    format!(
        "{}.{}.{}.{}",
        value >> 24,
        (value >> 16) & 0xff,
        (value >> 8) & 0xff,
        value & 0xff
    )
}

/// Writes IPv6 in RFC 5952 form: lowercase, no leading zeros, the longest run of two or more
/// zero groups (the first on a tie) written as `::`.
pub(super) fn format_v6(value: u128) -> String {
    let groups: Vec<u16> = (0..8)
        .map(|i| ((value >> (112 - 16 * i)) & 0xffff) as u16)
        .collect();
    let (mut best_start, mut best_len) = (0, 0);
    let mut i = 0;
    while i < 8 {
        if groups[i] == 0 {
            let start = i;
            while i < 8 && groups[i] == 0 {
                i += 1;
            }
            if i - start > best_len {
                (best_start, best_len) = (start, i - start);
            }
        } else {
            i += 1;
        }
    }
    let hex = |slice: &[u16]| -> String {
        slice
            .iter()
            .map(|g| format!("{g:x}"))
            .collect::<Vec<_>>()
            .join(":")
    };
    if best_len < 2 {
        return hex(&groups);
    }
    format!(
        "{}::{}",
        hex(&groups[..best_start]),
        hex(&groups[best_start + best_len..])
    )
}
