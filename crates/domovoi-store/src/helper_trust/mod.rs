//! Helper key pins, the latest attestation outcome per host and the high-water marks: write
//! application and the read port. The tables are `helper_keys`, `helper_attestation` and
//! `helper_high_water`.

mod apply;
mod read;

pub(crate) use apply::apply;
