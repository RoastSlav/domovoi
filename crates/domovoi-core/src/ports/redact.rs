//! Secret redaction.

/// Removes secrets from text before it is logged, stored, sent to a model or posted to chat
/// (INV-5).
pub trait Redactor: Send + Sync {
    /// Returns `text` with every known secret replaced by a fixed marker. Must be idempotent:
    /// redacting already-redacted text changes nothing.
    fn redact(&self, text: &str) -> String;
}
