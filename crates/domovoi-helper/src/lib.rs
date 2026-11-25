//! The root helper runtime of Domovoi: connection adoption, the closed request dispatcher, the
//! start checks, the host inspection adapter, the isolation self-test and the repair command.
//!
//! The helper spawns no process. Pure types, the platform tier and the check evaluators live in
//! `domovoi-core`; this crate holds everything that touches the host.

#[cfg(unix)]
pub mod adopt;
pub mod attest;
#[cfg(unix)]
pub mod inspect;
#[cfg(unix)]
pub mod platform;
#[cfg(unix)]
pub mod probe;
#[cfg(unix)]
pub mod repair;
#[cfg(unix)]
pub mod selftest;
#[cfg(unix)]
pub mod serve;
#[cfg(unix)]
pub mod start_check;
#[cfg(unix)]
pub mod state;
