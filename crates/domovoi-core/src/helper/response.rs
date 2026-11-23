//! The helper's answers.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::group::HelperGroup;
use crate::identity::{PathFault, RefusalToken};
use crate::isolation::{PlatformRecordState, PlatformTier};

/// Why a group's operation was not served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The group is not enabled in `helper.toml`.
    GroupDisabled,
    /// The group is enabled but nothing implements it yet.
    NotImplemented,
    /// The platform's isolation checks do not allow it.
    PlatformBlocked,
    /// A write group on an untested platform needs the owner's per-host opt-in.
    WriteGroupNeedsOptIn,
}

/// The answer to one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum HelperResponse {
    /// The helper is alive.
    Health {
        /// The helper protocol number.
        protocol: u16,
        /// The build that is running.
        build: String,
        /// The serving process's id.
        pid: u32,
    },
    /// The enabled groups and what the platform allows.
    Groups {
        /// Groups the helper will serve (write groups are never among them yet).
        enabled: Vec<HelperGroup>,
        /// Groups named in `helper.toml` but ignored, such as every write group.
        ignored: Vec<HelperGroup>,
        /// Whether the owner switched write groups on for untested platforms.
        write_opt_in: bool,
        /// The tier recorded by the last installer self-test.
        platform_tier: PlatformTier,
        /// Whether that record exists and matches the running kernel.
        platform_record: PlatformRecordState,
    },
    /// A group's operation is not served.
    Unavailable {
        /// The group asked.
        group: HelperGroup,
        /// Why not.
        reason: UnavailableReason,
    },
    /// The request was refused before it was understood.
    Refused {
        /// Why.
        token: RefusalToken,
    },
    /// A start check failed; every request is answered with this.
    StartRefused {
        /// Why.
        token: RefusalToken,
        /// Every offending path with found and expected values.
        faults: Vec<PathFault>,
    },
}

/// One answer on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperResponseFrame {
    /// The helper protocol number.
    pub v: u16,
    /// The id of the request being answered.
    pub id: Uuid,
    /// The answer.
    pub response: HelperResponse,
}
