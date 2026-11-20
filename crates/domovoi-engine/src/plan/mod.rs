//! The plan validator: the one filter between model text and a proposed step.
//!
//! A suggestion from the model is only data. This module checks each suggested step against
//! the hub-owned capability contract, the skill's manifest, the operation's pinned scope and
//! the identity the target resolver can prove, and tells the model why a step was refused.
//! It never writes, never decides approval and never sees an authorization: it is an early
//! filter and a feedback channel, and the operation writer still checks everything again.

mod refusal;
mod resolve;
mod schema;
mod scope;
mod validate;

#[cfg(test)]
mod schema_tests;

pub use refusal::{PlanRefusal, PlanRefusalCode};
pub use resolve::resolve_named_target;
pub use schema::{SchemaProblem, check_schema};
pub use scope::{PlanScope, PriorRun};
pub use validate::{PlanValidator, PlanVerdict, PlannedStep, is_executable};
