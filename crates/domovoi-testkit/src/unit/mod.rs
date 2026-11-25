//! Fakes for the unit capabilities: a scripted relay, and a builder of `unit.identity` answers.

mod relay;

pub use relay::{FakeUnitRelay, identity_outputs};
