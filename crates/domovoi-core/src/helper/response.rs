//! The helper's answers.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use uuid::Uuid;

use super::attestation::{SignedAttestation, UnsignedToken};
use super::group::HelperGroup;
use super::read::{ContainerWire, DeviceRef, DockerEngineInfo, EventWire, ListenerWire, ReadToken};
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
    /// The engine probe: `None` when no socket answered (a `ReadFailed` carries the reason).
    DockerEngine {
        /// The engine found.
        engine: Option<DockerEngineInfo>,
    },
    /// Every container, sorted by name.
    DockerContainers {
        /// The containers.
        containers: Vec<ContainerWire>,
        /// Whether the list was cut at the cap.
        truncated: bool,
    },
    /// Container events in the window.
    DockerEvents {
        /// The events.
        events: Vec<EventWire>,
        /// Whether the list was cut at the cap.
        truncated: bool,
    },
    /// One container's inspect document, with every environment value removed.
    DockerInspect {
        /// The document.
        inspect: serde_json::Value,
    },
    /// One container's log text.
    DockerLogs {
        /// The text.
        text: String,
        /// Whether it was cut.
        truncated: bool,
    },
    /// The devices a scan found, already filtered by the allowlist.
    SmartScan {
        /// The devices.
        devices: Vec<DeviceRef>,
    },
    /// A SMART report.
    SmartReport {
        /// The raw `smartctl --json` text.
        json: String,
        /// The tool's exit code, a bit mask.
        exit_code: Option<i32>,
        /// Whether the text was cut.
        truncated: bool,
    },
    /// Kernel log lines.
    KernelLog {
        /// The lines.
        text: String,
        /// Whether the text was cut.
        truncated: bool,
    },
    /// Listening sockets.
    NetListeners {
        /// The sockets.
        listeners: Vec<ListenerWire>,
        /// Whether the list was cut at the cap.
        truncated: bool,
    },
    /// A read operation was understood and served but failed.
    ReadFailed {
        /// The group of the operation.
        group: HelperGroup,
        /// Why.
        token: ReadToken,
        /// At most 200 printable ASCII bytes, with no path outside `/dev`.
        detail: Option<String>,
    },
    /// A signed safety report.
    SafetyAttested {
        /// The report, its signature and the key that made it.
        attestation: SignedAttestation,
    },
    /// No signed report could be made; the token says why. Carries no signature.
    SafetyUnsigned {
        /// Why.
        token: UnsignedToken,
    },
    /// The helper's public attestation key.
    SafetyPublicKey {
        /// 32 bytes as 64 lowercase hex characters.
        public_key: String,
    },
    /// A unit operation succeeded: named string outputs, at most 16 KiB in all.
    Outputs {
        /// The outputs.
        outputs: BTreeMap<String, String>,
    },
    /// A unit was refused or a restart did not complete; carries the token of the unit
    /// refusal table and an optional detail (a path, a field name).
    UnitRefused {
        /// The token, such as `target_stale`.
        token: String,
        /// At most 200 printable characters.
        detail: Option<String>,
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
