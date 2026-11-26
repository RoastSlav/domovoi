//! Approval, authorization and plan identity types shared by operations.

mod approval;
mod authorization;
mod capability_plan;
mod containment_plan;
mod epoch;
mod package_plan;
mod plan;
mod principal;

pub use approval::{ApprovalChannel, ApprovalProvenance, HumanApproval, PlanBinding, PlanHash};
pub use authorization::{
    Authorization, AuthorizationKind, PermitError, PolicyPermit, PolicyPermitParts,
};
pub use capability_plan::CapabilityPlan;
pub use containment_plan::{ContainmentPlan, PlanAssurance};
pub use epoch::JournalEpoch;
pub use package_plan::{PackagePlan, job_plan_hash};
pub use plan::{ImplHash, PlanContract, PlanError, PlanTarget};
pub use principal::Principal;
