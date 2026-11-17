//! Web and chat approvals: the only places the hub constructs a [`HumanApproval`] (INV-1).
//!
//! Web callers reach this only after the session and CSRF checks passed; chat callers only
//! after the interaction authorizer found the platform user on the allowlist. Either way the
//! approval is an authenticated human's.

use domovoi_core::{
    ApprovalChannel, ApprovalProvenance, Authorization, HumanApproval, SkillName, UserId,
};
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
/// The approval by `user` (the hub account chat acts as) pressed by `platform_user` on
/// `platform` at `now`. Never carries a typed confirmation, so destructive actions can't be
/// approved this way (INV-6).
#[must_use]
pub fn chat_approval(
    user: UserId,
    platform: &str,
    platform_user: &str,
    now: Timestamp,
) -> HumanApproval {
    HumanApproval::new(
        user,
        ApprovalChannel::Chat {
            platform: platform.to_owned(),
            platform_user: platform_user.to_owned(),
        },
        now,
    )
}

/// The web authorization and its provenance: the surface is `web` and `session_hash` is the
/// session's stored one-way hash, never the cookie or a token.
#[must_use]
pub fn web_authorization(
    user: UserId,
    confirm_text: Option<&str>,
    skill: &SkillName,
    session_hash: &[u8; 32],
    now: Timestamp,
) -> (Authorization, ApprovalProvenance) {
    let approval = web_approval(user, confirm_text, skill, now);
    (
        Authorization::human(approval),
        ApprovalProvenance::new(Some(hex::encode(session_hash)), "web"),
    )
}

/// The chat authorization and its provenance: the surface is `chat:<platform>` and there is
/// no session reference.
#[must_use]
pub fn chat_authorization(
    user: UserId,
    platform: &str,
    platform_user: &str,
    now: Timestamp,
) -> (Authorization, ApprovalProvenance) {
    let approval = chat_approval(user, platform, platform_user, now);
    (
        Authorization::human(approval),
        ApprovalProvenance::new(None, format!("chat:{platform}")),
    )
}
