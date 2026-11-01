//! Web approvals: the only place the web surface constructs a [`HumanApproval`] (INV-1).
//!
//! Callers reach this only after the session and CSRF checks passed, so the approval is
//! always an authenticated human's.

use domovoi_core::{ApprovalChannel, HumanApproval, SkillName, UserId};
use jiff::Timestamp;

/// The approval by `user` at `now` for an action running `skill`. It counts as typed
/// confirmation (needed for destructive skills, INV-6) only when `confirm_text` is exactly
/// the skill's name.
#[must_use]
pub fn web_approval(
    user: UserId,
    confirm_text: Option<&str>,
    skill: &SkillName,
    now: Timestamp,
) -> HumanApproval {
    let typed_confirmation = confirm_text == Some(skill.as_str());
    HumanApproval::new(user, ApprovalChannel::Web { typed_confirmation }, now)
}
