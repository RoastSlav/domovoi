//! Problems found while loading a configuration file.

use std::fmt;

/// How serious a [`ConfigIssue`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    /// The configuration cannot be used until this is fixed.
    Error,
    /// The configuration loads, but something should be fixed.
    Warning,
}

/// One problem in a configuration file, located by its TOML path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    /// Dotted TOML path such as `llm.primary.model` or `probes[0].target`; empty for
    /// problems with the whole file.
    pub path: String,
    /// What is wrong and how to fix it.
    pub message: String,
    /// Whether loading fails because of this issue.
    pub severity: IssueSeverity,
}

impl ConfigIssue {
    /// An error at `path`.
    #[must_use]
    pub fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            severity: IssueSeverity::Error,
        }
    }

    /// A warning at `path`.
    #[must_use]
    pub fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            severity: IssueSeverity::Warning,
        }
    }

    /// Whether this issue makes loading fail.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == IssueSeverity::Error
    }
}

impl fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            IssueSeverity::Error => "error",
            IssueSeverity::Warning => "warning",
        };
        if self.path.is_empty() {
            write!(f, "{level}: {}", self.message)
        } else {
            write!(f, "{level}: {}: {}", self.path, self.message)
        }
    }
}

/// Converts a deserialization error into an issue at the path where it happened.
pub(crate) fn from_path_error(err: &serde_path_to_error::Error<toml::de::Error>) -> ConfigIssue {
    let path = err.path().to_string();
    let path = if path == "." { String::new() } else { path };
    let raw = err.inner().message().trim().to_owned();
    ConfigIssue::error(path, rewrite(&raw))
}

fn rewrite(message: &str) -> String {
    if let Some(rest) = message.strip_prefix("unknown field ") {
        let expected = rest
            .split_once(", expected ")
            .map(|(_, expected)| format!(" (expected {expected})"))
            .unwrap_or_default();
        return format!("unknown key; remove it or check the spelling{expected}");
    }
    if let Some(field) = message.strip_prefix("missing field ") {
        return format!("missing required key {field}; add it");
    }
    message.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_unknown_and_missing_field_messages() {
        assert_eq!(
            rewrite("unknown field `bnd`, expected `bind` or `public_url`"),
            "unknown key; remove it or check the spelling (expected `bind` or `public_url`)"
        );
        assert_eq!(
            rewrite("missing field `hub_url`"),
            "missing required key `hub_url`; add it"
        );
        assert_eq!(rewrite("invalid type"), "invalid type");
    }

    #[test]
    fn display_includes_level_and_path() {
        assert_eq!(
            ConfigIssue::error("web.bind", "bad").to_string(),
            "error: web.bind: bad"
        );
        assert_eq!(ConfigIssue::warning("", "w").to_string(), "warning: w");
    }
}
