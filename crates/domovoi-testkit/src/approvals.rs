//! Human approvals for tests.
//!
//! Outside tests only the hub's auth layer may construct a `HumanApproval` (INV-1,
//! ADR 0007); `xtask arch-check` allows testkit as the one other place.

use domovoi_core::{ApprovalChannel, HumanApproval, UserId};

use crate::build::T0;

/// An approval from `user` through the web UI without typed confirmation, at [`T0`].
#[must_use]
pub fn human(user: UserId) -> HumanApproval {
    HumanApproval::new(
        user,
        ApprovalChannel::Web {
            typed_confirmation: false,
        },
        T0,
    )
}

/// An approval from `user` through the web UI with typed confirmation, which destructive
/// actions require, at [`T0`].
#[must_use]
pub fn web_typed(user: UserId) -> HumanApproval {
    HumanApproval::new(
        user,
        ApprovalChannel::Web {
            typed_confirmation: true,
        },
        T0,
    )
}

/// An approval from an allowlisted chat user, at [`T0`].
#[must_use]
pub fn chat(user: UserId, platform: &str, platform_user: &str) -> HumanApproval {
    HumanApproval::new(
        user,
        ApprovalChannel::Chat {
            platform: platform.to_owned(),
            platform_user: platform_user.to_owned(),
        },
        T0,
    )
}
