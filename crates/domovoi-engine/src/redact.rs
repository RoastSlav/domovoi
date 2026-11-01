//! The production [`Redactor`]: registered secret values plus patterns for common
//! credentials.

use domovoi_core::ports::Redactor;
use regex::Regex;
use secrecy::{ExposeSecret, SecretString};

/// What replaces every redacted value.
pub const REDACTED: &str = "[REDACTED]";

/// Credential shapes replaced wherever they appear. The last one keeps the key and
/// replaces only the value.
const PATTERNS: [&str; 9] = [
    r"Bearer\s+[A-Za-z0-9._~+/-]+=*",
    r"sk-[A-Za-z0-9_-]{20,}",
    r"ghp_[A-Za-z0-9]{36}",
    r"github_pat_[A-Za-z0-9_]{50,}",
    r"AKIA[0-9A-Z]{16}",
    r"xox[baprs]-[A-Za-z0-9-]{10,}",
    r"https://discord(app)?\.com/api/webhooks/\S+",
    r"eyJ[\w-]+\.[\w-]+\.[\w-]+",
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
];

/// `key = value` pairs whose key names a secret.
const KEY_VALUE: &str = r"(?i)((?:password|passwd|secret|token|api[_-]?key)\s*[=:]\s*)\S+";

/// Replaces registered secret values (longest first) and credential patterns with
/// [`REDACTED`]. Idempotent: redacting twice gives the same text.
pub struct PatternRedactor {
    secrets: Vec<SecretString>,
    patterns: Vec<Regex>,
    key_value: Option<Regex>,
}

impl std::fmt::Debug for PatternRedactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PatternRedactor")
            .field("secrets", &self.secrets.len())
            .field("patterns", &self.patterns.len())
            .finish()
    }
}

impl PatternRedactor {
    /// A redactor for `secrets`, typically every secret in the configuration. Empty values
    /// are ignored.
    #[must_use]
    pub fn new(secrets: impl IntoIterator<Item = SecretString>) -> Self {
        let mut secrets: Vec<SecretString> = secrets
            .into_iter()
            .filter(|s| !s.expose_secret().is_empty())
            .collect();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.expose_secret().len()));
        Self {
            secrets,
            patterns: PATTERNS.iter().filter_map(|p| Regex::new(p).ok()).collect(),
            key_value: Regex::new(KEY_VALUE).ok(),
        }
    }
}

impl Default for PatternRedactor {
    fn default() -> Self {
        Self::new(std::iter::empty())
    }
}

impl Redactor for PatternRedactor {
    fn redact(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for secret in &self.secrets {
            let secret = secret.expose_secret();
            if out.contains(secret) {
                out = out.replace(secret, REDACTED);
            }
        }
        for pattern in &self.patterns {
            out = pattern.replace_all(&out, REDACTED).into_owned();
        }
        if let Some(key_value) = &self.key_value {
            out = key_value
                .replace_all(&out, format!("${{1}}{REDACTED}"))
                .into_owned();
        }
        out
    }
}
