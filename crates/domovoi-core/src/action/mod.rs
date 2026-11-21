//! Actions: proposals to run write or destructive skills, approved only by humans.

mod approval;
mod authorization;
mod epoch;
mod package_plan;
mod plan;
mod request;
mod stored;
mod transition;

pub use approval::{ApprovalChannel, ApprovalProvenance, HumanApproval, PlanBinding, PlanHash};
pub use authorization::{
    Authorization, AuthorizationKind, PermitError, PolicyPermit, PolicyPermitParts,
};
pub use epoch::JournalEpoch;
pub use package_plan::{PackagePlan, job_plan_hash};
pub use plan::{ImplHash, PlanContract, PlanError, PlanTarget};
pub use request::{ActionRequest, ActionStatus, ApprovalPlan, PlanFamily, Principal, StoredAction};
pub use stored::{ApprovalEvidenceError, StoredActionError};
pub use transition::{ActionChange, ActionError};
