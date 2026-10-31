//! Agent runtime tests against a test hub on loopback, with a fake clock.

#[cfg(test)]
#[path = "../common/skills.rs"]
mod common;
#[cfg(test)]
mod docker;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod hub;
#[cfg(test)]
mod link;
#[cfg(test)]
mod requests;
#[cfg(test)]
mod runtime;
#[cfg(test)]
mod scheduler;
#[cfg(test)]
mod spool;
