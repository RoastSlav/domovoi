//! The helper's unit operations: the `busctl` adapter against recorded replies, and the
//! identity and restart steps against a stateful fake manager. No test touches a real systemd.
#![cfg(unix)]

#[cfg(test)]
mod adapter;
#[cfg(test)]
mod gate;
#[cfg(test)]
mod restart;
#[cfg(test)]
mod world;
