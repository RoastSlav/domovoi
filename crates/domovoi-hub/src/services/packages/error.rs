//! Why a package command or read was refused.

use domovoi_core::ports::StoreError;
use domovoi_engine::packages::PackageError;

use crate::packages::ImportError;

/// The text of a refusal when no password came with a request that needs one.
pub const PASSWORD_REQUIRED_TEXT: &str = "installing a package needs your password";

/// Why a package command or read was refused. Nothing changed unless the variant says so.
#[derive(Debug, thiserror::Error)]
pub enum PackageCommandError {
    /// `[packages] enabled` is off.
    #[error("packages are not enabled on this hub")]
    Disabled,
    /// A package job is approved in the web UI only.
    #[error("packages are installed from the web UI only")]
    WebOnly,
    /// The request is malformed: host count, repeated host or an unreadable field.
    #[error("{0}")]
    BadRequest(String),
    /// The request carried no password.
    #[error("installing a package needs your password")]
    PasswordRequired,
    /// Enabling or disabling a package's skills carried no password.
    #[error("{} a package's skills needs your password", if *.enable { "enabling" } else { "disabling" })]
    SkillsPasswordRequired {
        /// Whether the request was to enable.
        enable: bool,
    },
    /// The password is wrong.
    #[error("wrong password")]
    BadPassword,
    /// Too many wrong passwords from this address.
    #[error("too many attempts; try again in {retry_after_secs} s")]
    TooMany {
        /// Seconds until the next attempt.
        retry_after_secs: i64,
    },
    /// The typed text is not the package's name.
    #[error("type the package name to confirm")]
    ConfirmationMismatch,
    /// No such host.
    #[error("no such host")]
    NoSuchHost,
    /// The import or delete was refused.
    #[error(transparent)]
    Import(#[from] ImportError),
    /// The install service refused the command.
    #[error(transparent)]
    Install(#[from] PackageError),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}
