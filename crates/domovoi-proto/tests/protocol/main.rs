//! Integration tests for the wire protocol: round trips, limits, versions and the
//! snapshot-pinned JSON of every message kind.

#[cfg(test)]
mod auth;
#[cfg(test)]
mod roundtrip;
#[cfg(test)]
mod samples;
#[cfg(test)]
mod snapshots;
#[cfg(test)]
mod version;
