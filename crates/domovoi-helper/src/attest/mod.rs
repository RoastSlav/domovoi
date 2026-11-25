//! The helper's safety attestation: key, state, route observation and the signed report.
//!
//! `safety.attest` answers a challenge with a report of the effective safety set, signed by a key
//! only this process can read. The pure parts (the state format, generation planning, the `ip`
//! parsers, route observation and the apply-time guard) build on every platform; the parts that
//! touch the state directory (the lock, the key and state files and the lifecycle commands) are
//! Unix only.

pub mod compute;
pub mod endpoints;
pub mod guard;
pub mod ip_json;
pub mod ip_source;
pub mod keys;
pub mod observe;
pub mod state;

#[cfg(unix)]
pub mod current;
#[cfg(unix)]
pub mod lifecycle;
#[cfg(unix)]
mod lock;
#[cfg(unix)]
mod run;

#[cfg(unix)]
pub use run::{AttestEnv, AttestOutcome, AttestStage, public_key, safety_attest};

/// The key file's name inside the state directory.
pub const KEY_FILE: &str = "attest.key";
/// The state file's name inside the state directory.
pub const STATE_FILE: &str = "state";
/// The directory that keeps what rotation and recovery retired: public keys and old states only.
pub const RETIRED_DIR: &str = "retired";
/// Where the kernel's boot id is read.
pub const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
