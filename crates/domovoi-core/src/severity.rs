//! How serious a signal or incident is.

use std::fmt;

/// Severity of a signal or incident, ordered `Info < Warning < Critical`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Worth recording; needs no attention.
    Info,
    /// Needs attention soon.
    Warning,
    /// Needs attention now.
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_orders_info_below_warning_below_critical() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Critical);
    }

    #[test]
    fn severity_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Severity::Critical).unwrap(),
            "\"critical\""
        );
    }
}
