//! The stored form of a rule: a versioned JSON envelope, and the hash over a rule set.
//!
//! The envelope is the only place the shape of [`Rule`] is frozen for storage. A change to
//! the evaluator's types reaches stored data only through an explicit new `v`.

use domovoi_core::policy_store::StoredRule;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::policy::Rule;

/// The envelope version this build writes and reads.
const VERSION: u64 = 1;
/// What the rule-set hash starts with.
const HASH_PREFIX: &str = "policy-rules-v1\n";

/// Why a stored rule could not be read back.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// The envelope names a version this build does not know.
    #[error("unsupported rule envelope version {0}")]
    Version(u64),
    /// The body is not a valid envelope or rule.
    #[error("rule body is not valid: {0}")]
    Json(String),
    /// The projected columns disagree with the body.
    #[error("rule {rule}: stored columns disagree with the body")]
    ProjectionMismatch {
        /// The rule whose row is inconsistent.
        rule: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    v: u64,
    rule: Rule,
}

/// `{"v":1,"rule":<Rule>}`, compact.
#[must_use]
pub fn encode_rule(rule: &Rule) -> String {
    // A `Rule` holds only strings, numbers, booleans and maps with string keys, so
    // serialization cannot fail; an empty body would be refused on read.
    let value = serde_json::to_value(rule).unwrap_or(serde_json::Value::Null);
    serde_json::json!({ "v": VERSION, "rule": value }).to_string()
}

/// Reads one envelope. A version other than 1, an extra top-level key or a bad rule is an
/// error.
pub fn decode_rule(body: &str) -> Result<Rule, CodecError> {
    let probe: serde_json::Value =
        serde_json::from_str(body).map_err(|e| CodecError::Json(e.to_string()))?;
    if let Some(v) = probe.get("v").and_then(serde_json::Value::as_u64)
        && v != VERSION
    {
        return Err(CodecError::Version(v));
    }
    let envelope: Envelope =
        serde_json::from_value(probe).map_err(|e| CodecError::Json(e.to_string()))?;
    if envelope.v != VERSION {
        return Err(CodecError::Version(envelope.v));
    }
    Ok(envelope.rule)
}

/// The rows for `rules`: ordinal is the position, projections are copied from the rule.
#[must_use]
pub fn stored_rules(rules: &[Rule]) -> Vec<StoredRule> {
    rules
        .iter()
        .enumerate()
        .map(|(ordinal, rule)| StoredRule {
            rule_id: rule.id.as_str().to_owned(),
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
            enabled: rule.enabled,
            shadow: rule.shadow,
            locked: rule.locked,
            body: encode_rule(rule),
        })
        .collect()
}

/// Decodes `stored` in ordinal order and checks the projections against each body.
pub fn decode_rules(stored: &[StoredRule]) -> Result<Vec<Rule>, CodecError> {
    let mut ordered: Vec<&StoredRule> = stored.iter().collect();
    ordered.sort_by_key(|row| row.ordinal);
    ordered
        .into_iter()
        .map(|row| {
            let rule = decode_rule(&row.body)?;
            let agrees = rule.id.as_str() == row.rule_id
                && rule.enabled == row.enabled
                && rule.shadow == row.shadow
                && rule.locked == row.locked;
            if agrees {
                Ok(rule)
            } else {
                Err(CodecError::ProjectionMismatch {
                    rule: row.rule_id.clone(),
                })
            }
        })
        .collect()
}

/// Lowercase hex SHA-256 of the prefix, then each rule's id and body, each followed by a
/// newline, in ordinal order.
#[must_use]
pub fn rules_hash(stored: &[StoredRule]) -> String {
    let mut ordered: Vec<&StoredRule> = stored.iter().collect();
    ordered.sort_by_key(|row| row.ordinal);
    let mut hasher = Sha256::new();
    hasher.update(HASH_PREFIX.as_bytes());
    for row in ordered {
        hasher.update(row.rule_id.as_bytes());
        hasher.update(b"\n");
        hasher.update(row.body.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}
