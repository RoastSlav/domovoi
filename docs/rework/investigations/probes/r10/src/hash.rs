//! Domain-separated SHA-256 over LF-terminated lines (04b section 3).

use sha2::{Digest, Sha256};

/// Domain prefix line of the config hash.
pub const CONFIG_PREFIX: &str = "domovoi-safety-config-v1";
/// Domain prefix line of the route-state hash.
pub const ROUTES_PREFIX: &str = "domovoi-safety-routes-v1";
/// Domain prefix of the signing input (the prefix line, then the report bytes).
pub const SIGNATURE_PREFIX: &str = "domovoi-safety-attest-v1";
/// Placeholder written where a hash cannot be computed in an incomplete report (a harness
/// choice; the contract does not say what the key carries).
pub const HASH_SENTINEL: [u8; 32] = [0; 32];

/// The exact bytes that are hashed: the prefix line, then each line, every one LF-terminated.
#[must_use]
pub fn domain_bytes(prefix: &str, lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(prefix.as_bytes());
    out.push(b'\n');
    for l in lines {
        out.extend_from_slice(l.as_bytes());
        out.push(b'\n');
    }
    out
}

/// SHA-256 of [`domain_bytes`].
#[must_use]
pub fn domain_hash(prefix: &str, lines: &[String]) -> [u8; 32] {
    Sha256::digest(domain_bytes(prefix, lines)).into()
}

/// SHA-256 of raw bytes.
#[must_use]
pub fn sha256(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

/// Strict lowercase 64-hex to 32 bytes.
#[must_use]
pub fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    hex::decode(s).ok()?.try_into().ok()
}
