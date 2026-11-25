//! Attestation tests that need Unix modes, ownership and `flock`: the lock, the key and state
//! files, the lifecycle commands and `safety.attest` over a real directory. They run in WSL or
//! on Linux and do nothing on Windows.
#![cfg(unix)]

#[cfg(test)]
#[path = "../support/mod.rs"]
mod support;

#[cfg(test)]
mod concurrency;
#[cfg(test)]
mod conn;
#[cfg(test)]
mod lifecycle;
#[cfg(test)]
mod run;
#[cfg(test)]
mod serve_wiring;
#[cfg(test)]
mod tokens;
#[cfg(test)]
mod world;
