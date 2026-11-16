//! Actions: proposals to run write or destructive skills, approved only by humans.

mod approval;
mod epoch;
mod request;
mod transition;

pub use approval::{ApprovalChannel, HumanApproval, PlanBinding, PlanHash};
pub use epoch::JournalEpoch;
pub use request::{ActionRequest, ActionStatus, Principal, StoredAction};
pub use transition::{ActionChange, ActionError};
