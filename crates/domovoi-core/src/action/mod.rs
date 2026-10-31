//! Actions: proposals to run write or destructive skills, approved only by humans.

mod approval;
mod request;
mod transition;

pub use approval::{ApprovalChannel, HumanApproval, PlanHash};
pub use request::{ActionRequest, ActionStatus, Principal};
pub use transition::{ActionChange, ActionError};
