//! The `[smart] devices` allowlist: a pattern is literal `a-z`, `0-9` and `/` characters plus the
//! single-character classes `[a-z]` and `[0-9]`, and it matches a device only as a whole string.

use domovoi_core::helper::read::DeviceRef;

/// One position of a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Literal(u8),
    Letter,
    Digit,
}

/// Reads `pattern` into positions, or `None` when it is outside the grammar.
fn compile(pattern: &str) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut rest = pattern.as_bytes();
    while let Some(&first) = rest.first() {
        if let Some(after) = rest.strip_prefix(b"[a-z]") {
            parts.push(Part::Letter);
            rest = after;
        } else if let Some(after) = rest.strip_prefix(b"[0-9]") {
            parts.push(Part::Digit);
            rest = after;
        } else if first.is_ascii_lowercase() || first.is_ascii_digit() || first == b'/' {
            parts.push(Part::Literal(first));
            rest = &rest[1..];
        } else {
            return None;
        }
    }
    (!parts.is_empty()).then_some(parts)
}

/// Whether `text` is matched, in full, by `pattern`. A pattern outside the grammar matches nothing.
#[must_use]
pub fn matches(pattern: &str, text: &str) -> bool {
    let Some(parts) = compile(pattern) else {
        return false;
    };
    let bytes = text.as_bytes();
    parts.len() == bytes.len()
        && parts.iter().zip(bytes).all(|(part, byte)| match part {
            Part::Literal(want) => want == byte,
            Part::Letter => byte.is_ascii_lowercase(),
            Part::Digit => byte.is_ascii_digit(),
        })
}

/// Whether `device` matches any of `patterns`. An empty list admits nothing.
#[must_use]
pub fn allows(patterns: &[String], device: &DeviceRef) -> bool {
    patterns
        .iter()
        .any(|pattern| matches(pattern, device.as_str()))
}
