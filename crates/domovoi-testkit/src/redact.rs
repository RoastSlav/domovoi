//! A redactor for a fixed list of secrets.

use domovoi_core::ports::Redactor;

/// The marker that replaces each secret.
pub const REDACTED: &str = "[REDACTED]";

/// A [`Redactor`] that replaces every occurrence of configured secret strings with
/// [`REDACTED`]. Empty strings are ignored; longer secrets are replaced first so a secret
/// that contains another is removed whole.
#[derive(Debug, Clone, Default)]
pub struct ListRedactor {
    secrets: Vec<String>,
}

impl ListRedactor {
    /// A redactor for `secrets`.
    #[must_use]
    pub fn new<I, S>(secrets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut secrets: Vec<String> = secrets
            .into_iter()
            .map(Into::into)
            .filter(|s| !s.is_empty())
            .collect();
        secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        secrets.dedup();
        Self { secrets }
    }
}

impl Redactor for ListRedactor {
    fn redact(&self, text: &str) -> String {
        self.secrets.iter().fold(text.to_owned(), |acc, secret| {
            acc.replace(secret.as_str(), REDACTED)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_redactor_replaces_longest_secrets_first_and_is_idempotent() {
        let redactor = ListRedactor::new(["tok", "tok-123", ""]);
        let once = redactor.redact("key=tok-123 and tok");
        assert_eq!(once, "key=[REDACTED] and [REDACTED]");
        assert_eq!(redactor.redact(&once), once);
        assert_eq!(redactor.redact("nothing here"), "nothing here");
    }
}
