//! Canonical encoding of audit rows, the legacy digest and the anchor hash.

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Hash;
use crate::ports::repos::AuditEvent;

/// Commits to every audit row that existed before the hash chain was introduced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditAnchor {
    /// A fresh identifier written when the anchor was created.
    pub install_id: String,
    /// The highest legacy row sequence number, `0` on a fresh install.
    pub legacy_last_id: u64,
    /// How many legacy rows the digest covers.
    pub legacy_count: u64,
    /// The SHA-256 digest over every legacy row.
    pub legacy_digest: Hash,
}

impl AuditAnchor {
    /// The anchor's own hash, which the first chained row names as its `prev_hash`.
    #[must_use]
    pub fn hash(&self) -> Hash {
        let install = self.install_id.as_bytes();
        let mut hasher = Sha256::new();
        hasher.update(b"domovoi-audit-anchor-v1");
        hasher.update(length_u32(install.len()));
        hasher.update(install);
        hasher.update(self.legacy_last_id.to_be_bytes());
        hasher.update(self.legacy_count.to_be_bytes());
        hasher.update(self.legacy_digest);
        hasher.finalize().into()
    }
}

/// The compact JSON object hashed for one audit row: five keys in alphabetical order, with every
/// object's keys recursively sorted.
#[must_use]
pub fn canonical_json(event: &AuditEvent) -> String {
    format!(
        "{{\"actor\":{},\"at\":{},\"detail\":{},\"kind\":{},\"subject\":{}}}",
        json_string(&event.actor),
        event.at.as_millisecond(),
        canonical_value(&event.detail),
        json_string(&event.kind),
        json_string(&event.subject),
    )
}

fn canonical_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => json_string(text),
        Value::Array(items) => {
            let mut out = String::from("[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&canonical_value(item));
            }
            out.push(']');
            out
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            let mut out = String::from("{");
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&json_string(key));
                out.push(':');
                out.push_str(&canonical_value(&map[key]));
            }
            out.push('}');
            out
        }
    }
}

/// A JSON string literal, compact and with non-ASCII text kept as UTF-8.
fn json_string(text: &str) -> String {
    // Serializing a string cannot fail; the fallback keeps the signature total without `unwrap`.
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

/// The hash of one chained audit row:
/// `SHA-256("domovoi-audit-v1" ‖ u64be(seq) ‖ prev ‖ canonical_json(event))`.
#[must_use]
pub fn entry_hash(seq: u64, prev: &Hash, event: &AuditEvent) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update(b"domovoi-audit-v1");
    hasher.update(seq.to_be_bytes());
    hasher.update(prev);
    hasher.update(canonical_json(event).as_bytes());
    hasher.finalize().into()
}

/// A streaming digest over legacy rows, in ascending sequence order.
#[derive(Debug, Clone, Default)]
pub struct LegacyDigest {
    hasher: Sha256,
    count: u64,
}

impl LegacyDigest {
    /// Starts an empty digest; with no rows its result is the SHA-256 of the empty string.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one legacy row. The stored `detail` text is parsed first, so key order in the text
    /// does not matter.
    pub fn update(&mut self, seq: u64, event: &AuditEvent) {
        let canonical = canonical_json(event);
        let bytes = canonical.as_bytes();
        self.hasher.update(seq.to_be_bytes());
        self.hasher.update(length_u32(bytes.len()));
        self.hasher.update(bytes);
        self.count += 1;
    }

    /// The number of rows added and their digest.
    #[must_use]
    pub fn finish(self) -> (u64, Hash) {
        (self.count, self.hasher.finalize().into())
    }
}

fn length_u32(length: usize) -> [u8; 4] {
    u32::try_from(length).unwrap_or(u32::MAX).to_be_bytes()
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use serde_json::json;

    use super::*;

    fn event(actor: &str, at: i64, kind: &str, subject: &str, detail: Value) -> AuditEvent {
        AuditEvent {
            at: Timestamp::from_millisecond(at).unwrap(),
            actor: actor.to_owned(),
            kind: kind.to_owned(),
            subject: subject.to_owned(),
            detail,
        }
    }

    fn empty_digest() -> Hash {
        let hasher = Sha256::new();
        hasher.finalize().into()
    }

    #[test]
    fn canonical_json_sorts_keys_recursively_and_is_compact() {
        let event = event(
            "user:0190a0a0-0000-7000-8000-000000000001",
            1_700_000_000_123,
            "action.approved",
            "action:0190a0a0-0000-7000-8000-000000000002",
            json!({"b": 1, "a": [true, null]}),
        );
        assert_eq!(
            canonical_json(&event),
            "{\"actor\":\"user:0190a0a0-0000-7000-8000-000000000001\",\"at\":1700000000123,\
             \"detail\":{\"a\":[true,null],\"b\":1},\"kind\":\"action.approved\",\
             \"subject\":\"action:0190a0a0-0000-7000-8000-000000000002\"}"
        );
    }

    #[test]
    fn anchor_hash_matches_golden_vectors() {
        let fresh = AuditAnchor {
            install_id: "install-1".to_owned(),
            legacy_last_id: 0,
            legacy_count: 0,
            legacy_digest: empty_digest(),
        };
        assert_eq!(
            hex::encode(fresh.hash()),
            "9f0e006afbfa18e042a21bfb571b46792ee6a222f79fb448aeb0d026f93fd549"
        );

        let legacy = AuditAnchor {
            install_id: "install-2".to_owned(),
            legacy_last_id: 2,
            legacy_count: 2,
            legacy_digest: hex::decode(
                "9acfd54233b8d05352496b07960b45dcf81083510c76d9740e291df079d76cbe",
            )
            .unwrap()
            .try_into()
            .unwrap(),
        };
        assert_eq!(
            hex::encode(legacy.hash()),
            "41a8cf272a78ac1655662964bdd8858f6eebfaf5c0e14dc38bcf37208c79f6ad"
        );
    }

    #[test]
    fn entry_hash_matches_golden_vectors() {
        let anchor =
            hex::decode("9f0e006afbfa18e042a21bfb571b46792ee6a222f79fb448aeb0d026f93fd549")
                .unwrap()
                .try_into()
                .unwrap();
        let first = event(
            "user:0190a0a0-0000-7000-8000-000000000001",
            1_700_000_000_123,
            "action.approved",
            "action:0190a0a0-0000-7000-8000-000000000002",
            json!({"b": 1, "a": [true, null]}),
        );
        let first_hash = entry_hash(1, &anchor, &first);
        assert_eq!(
            hex::encode(first_hash),
            "a4f75c26c56d0ac324b1d31418ca4281388129b0eca508e3b0fca480ebfe0864"
        );

        let second = event(
            "system:actions",
            1_700_000_001_000,
            "action.dispatched",
            "action:0190a0a0-0000-7000-8000-000000000002",
            Value::Null,
        );
        assert_eq!(
            canonical_json(&second),
            "{\"actor\":\"system:actions\",\"at\":1700000001000,\"detail\":null,\
             \"kind\":\"action.dispatched\",\
             \"subject\":\"action:0190a0a0-0000-7000-8000-000000000002\"}"
        );
        assert_eq!(
            hex::encode(entry_hash(2, &first_hash, &second)),
            "fd2a3eb162101f8b87ea3904222b963f9d9a30120703b0a292dc0291da612436"
        );
    }

    #[test]
    fn legacy_digest_matches_golden_vector() {
        let mut digest = LegacyDigest::new();
        digest.update(
            1,
            &event(
                "user:u1",
                1_700_000_000_000,
                "login",
                "user:u1",
                Value::Null,
            ),
        );
        digest.update(
            2,
            &event(
                "system:actions",
                1_700_000_000_500,
                "action.expired",
                "action:a1",
                json!({"x": 1}),
            ),
        );
        let (count, hash) = digest.finish();
        assert_eq!(count, 2);
        assert_eq!(
            hex::encode(hash),
            "9acfd54233b8d05352496b07960b45dcf81083510c76d9740e291df079d76cbe"
        );
    }

    #[test]
    fn legacy_digest_of_no_rows_is_the_empty_digest() {
        let (count, hash) = LegacyDigest::new().finish();
        assert_eq!(count, 0);
        assert_eq!(
            hex::encode(hash),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
