//! The closed set of tools the model is offered, and the typed input of each.
//!
//! No tool approves, authorizes, runs or writes anything: the model only looks, suggests or
//! finishes. What a suggestion becomes is decided by deterministic code elsewhere.

mod call;
mod plan_input;
mod schema;
mod tool;

#[cfg(test)]
mod tests;

pub use call::{MetaArgError, MetaToolCall};
pub use plan_input::{ArgValue, PlanInput, PlanStepInput, RetryOfInput};
pub use tool::{MetaTool, ToolSet};
