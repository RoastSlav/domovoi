//! Storing policy: the unit of work's applier for `StateWrite::Policy`, the row types and the
//! read queries behind `PolicyRepo`.
//!
//! A published version is immutable: the applier never matches a published row with a
//! draft-only write, and triggers in the migration refuse raw changes to its rules.

mod apply;
pub(crate) mod queries;
pub(crate) mod rows;

pub(crate) use apply::apply;
