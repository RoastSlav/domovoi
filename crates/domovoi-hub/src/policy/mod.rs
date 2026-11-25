//! The permit issuer: the only code that turns an enforced policy `Allow` into a stored permit.
//!
//! A permit is built from an `Allow` that held every hard limit of the evaluator and every guard
//! here, and in no other case. The guards run in a fixed order and each one returns early, so a
//! permit is never built on a path that skipped one. Nothing in this directory reaches an agent:
//! the issuer reads, evaluates and answers; the engine records what it is handed.

mod guards;
mod issuer;

pub use issuer::{HubPermitIssuer, IssuerDeps, PermitSettings};
