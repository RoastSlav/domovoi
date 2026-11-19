//! The single-use token a chat approval carries. Only its SHA-256 hash is ever stored; the
//! plain token travels once, from the code that mints it to the card's button.

use sha2::{Digest, Sha256};

/// A fresh single-use chat token: 244 random bits, hex-encoded.
#[must_use]
pub fn mint_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    hex::encode(bytes)
}

/// The hash stored for `token`; the token itself is never stored.
#[must_use]
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Whether two digests are equal, in time independent of where they differ.
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Whether a chat approval carries the token whose hash is `stored`. Missing on either side
/// is false.
pub(crate) fn chat_token_ok(token: Option<&str>, stored: Option<&[u8; 32]>) -> bool {
    match (token, stored) {
        (Some(token), Some(stored)) => same(&token_hash(token), stored),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_helpers_behave_as_before_the_move() {
        let (a, b) = (mint_token(), mint_token());
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        let expected: [u8; 32] = Sha256::digest(a.as_bytes()).into();
        assert_eq!(token_hash(&a), expected);
        assert!(chat_token_ok(Some(&a), Some(&token_hash(&a))));
        assert!(!chat_token_ok(Some(&a), Some(&token_hash(&b))));
        assert!(!chat_token_ok(None, Some(&token_hash(&a))));
        assert!(!chat_token_ok(Some(&a), None));
    }
}
