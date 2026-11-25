//! The helper's read-only view of the container engine.
//!
//! [`calls`] lists the five `GET` requests the helper can ever send, [`client`] sends them
//! through `bollard` and is the only place in the crate that uses it, [`map`] turns engine
//! answers into the wire types and [`sanitize`] removes `Config.Env` values.
//!
//! Truncation: the adapter returns at most one item more than the cap
//! (`MAX_CONTAINERS + 1`, `MAX_EVENTS + 1`). A caller that sees more than the cap cuts the list
//! to the cap and marks the answer truncated. Logs are cut by the adapter itself at
//! `MAX_LOG_TEXT_BYTES`; a caller that sees exactly that many bytes (or a text ending at a
//! character boundary below it after a cut) treats it as possibly truncated.

pub mod calls;
pub mod client;
pub mod map;
pub mod sanitize;

pub use client::BollardDockerRead;
