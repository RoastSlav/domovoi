//! Settings managed from the web UI (ADR 0017).
//!
//! Settings come from four layers, lowest first: built-in defaults, values saved here,
//! `hub.toml`, `DOMOVOI_*` variables. A save:
//!
//! 1. is refused with a conflict when the stored revision moved on;
//! 2. is applied to a copy of the stored document (secrets as keep, set or clear);
//! 3. needs the admin's password again when it widens trust (chat approvers, trusted
//!    proxies, webhooks, any secret);
//! 4. is refused for a field set in the file or the environment;
//! 5. is validated with every layer merged, returning every issue with its path;
//! 6. stores the document and the sealed secrets in one transaction, keeping the
//!    previous document for rollback;
//! 7. is audited as `settings.changed`, without secret values;
//! 8. is applied by the `hub run` supervisor restarting the runtime, which answers before
//!    it restarts, and rolls back if the new settings do not start.
//!
//! Secrets entered here are encrypted with the key in `<data_dir>/secret.key`. Only this
//! module writes the `config`, `config.previous`, `secret.*`, `host.*.checks` and
//! `last_apply` keys.

pub mod apply;
pub mod connection;
pub mod crypto;
pub mod patch;
pub mod service;
pub mod stored;

pub use apply::{ApplyRequest, ConfigSources, LoadError, load_effective, rollback};
pub use connection::{Target, TestResult};
pub use crypto::{CryptoError, SecretKey};
pub use service::{HostChecks, Saved, SettingsDeps, SettingsError, SettingsService, View};
pub use stored::host_checks_key;
