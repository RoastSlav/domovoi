//! Why a package operation failed, and the stable code the hub reads.

use domovoi_core::package::PackageFailureCode;

/// Longest attacker-controlled text quoted in a message, in characters.
const QUOTE_LIMIT: usize = 100;
/// Longest failure message sent to the hub, in characters.
const MESSAGE_LIMIT: usize = 500;

/// A failed package operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    /// The operation was refused or failed for the reason in `code`.
    #[error("{code}: {detail}")]
    Failed {
        /// The stable reason code.
        code: PackageFailureCode,
        /// A short explanation; any archive or file text in it is already escaped.
        detail: String,
    },
    /// A trusted key in `agent.toml` is unusable. Fatal when the agent starts.
    #[error("trusted key {id}: {reason}")]
    InvalidKey {
        /// The key id as written in the configuration.
        id: String,
        /// What is wrong with it.
        reason: String,
    },
    /// Package installation needs a Linux filesystem layer.
    #[error("package installation is not supported on this platform")]
    UnsupportedPlatform,
    /// A test hook stopped an install on purpose, with no cleanup, to simulate a crash.
    #[cfg(test)]
    #[error("injected crash")]
    InjectedCrash,
}

impl PackageError {
    /// A failure with `code` and a plain `detail`.
    #[must_use]
    pub fn new(code: PackageFailureCode, detail: impl Into<String>) -> Self {
        Self::Failed {
            code,
            detail: detail.into(),
        }
    }

    /// A `storage` failure for an I/O error, naming what was being done.
    #[must_use]
    pub fn io(doing: &str, error: &std::io::Error) -> Self {
        Self::new(
            PackageFailureCode::Storage,
            format!("{doing}: {}", error.kind()),
        )
    }

    /// The stable code for this failure.
    #[must_use]
    pub fn code(&self) -> PackageFailureCode {
        match self {
            Self::Failed { code, .. } => *code,
            Self::InvalidKey { .. } | Self::UnsupportedPlatform => PackageFailureCode::Storage,
            #[cfg(test)]
            Self::InjectedCrash => PackageFailureCode::Storage,
        }
    }

    /// The text for `capability_result.failed.message`: `"<code>: <detail>"`, at most 500
    /// characters.
    #[must_use]
    pub fn message(&self) -> String {
        let text = match self {
            Self::Failed { code, detail } => format!("{code}: {detail}"),
            other => format!("{}: {other}", other.code()),
        };
        text.chars().take(MESSAGE_LIMIT).collect()
    }
}

impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self::io("file operation failed", &error)
    }
}

/// Attacker-controlled `text` made safe to put in a message: escaped with `{:?}` and cut to
/// 100 characters.
#[must_use]
pub fn quote(text: &str) -> String {
    let escaped = format!("{text:?}");
    if escaped.chars().count() <= QUOTE_LIMIT {
        return escaped;
    }
    let mut cut: String = escaped.chars().take(QUOTE_LIMIT).collect();
    cut.push('…');
    cut
}

/// [`quote`] for raw bytes that may not be UTF-8.
#[must_use]
pub fn quote_bytes(bytes: &[u8]) -> String {
    quote(&String::from_utf8_lossy(bytes))
}
