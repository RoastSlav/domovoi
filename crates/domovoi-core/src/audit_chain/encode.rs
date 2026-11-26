//! Canonical encoding of audit rows and the anchor hash.

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::Hash;
use crate::ports::repos::AuditEvent;

/// The fixed starting point of the chain: the first row's `prev_hash` is the anchor's hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditAnchor {
    /// A fresh identifier written when the anchor was created.
    pub install_id: String,
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
    fn anchor_hash_matches_golden_vector() {
        let anchor = AuditAnchor {
            install_id: "install-1".to_owned(),
        };
        assert_eq!(
            hex::encode(anchor.hash()),
            "5136a8fd8fb5cf1c41f7847de5fd9ba486651b613a54a8e29cf2079ad40a8ca1"
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
}
