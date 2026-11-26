//! Rows 11 and 12 of the availability table for the host's own words: the ceiling, the helper
//! and what it offers, and the container runtime; and where a skill would actually run.

use domovoi_core::capability::{
    Availability, CapabilityContract, EffectClass, ExecutionPath, NotAllowedOnHost, TargetKind,
    UnavailableReason,
};
use domovoi_core::ports::{HelperState, HostWords, RunnerState};

use super::ceiling::{CeilingParse, covers, parse_ceiling};

/// Row 11: whether the agent's ceiling covers a non-`Read` contract.
pub(super) fn ceiling_allows(
    contract: &CapabilityContract,
    words: &HostWords,
) -> Result<(), Availability> {
    let absent = Availability::NotAllowedOnHost {
        reason: NotAllowedOnHost::CeilingAbsent,
    };
    match parse_ceiling(words.ceiling_summary.as_deref()) {
        CeilingParse::Absent | CeilingParse::Malformed { .. } => Err(absent),
        CeilingParse::Parsed(ceiling) if !ceiling.enabled => Err(absent),
        CeilingParse::Parsed(ceiling) if covers(&ceiling, &contract.id) => Ok(()),
        CeilingParse::Parsed(_) => Err(Availability::NotAllowedOnHost {
            reason: NotAllowedOnHost::CeilingEntryMissing,
        }),
    }
}

/// Rows 11 and 12 for a compiled contract: ceiling, helper, container runtime.
pub(super) fn host_allows(
    contract: &CapabilityContract,
    words: &HostWords,
) -> Result<(), Availability> {
    if contract.effect != EffectClass::Read {
        ceiling_allows(contract, words)?;
    }
    if contract.execution == ExecutionPath::Helper && words.helper == HelperState::Failed {
        return Err(Availability::Unavailable {
            reason: UnavailableReason::HelperError,
        });
    }
    // The helper offers an operation only when it says so: a read it serves, or a write whose
    // group is named and whose platform tier allows it.
    if contract.execution == ExecutionPath::Helper
        && matches!(contract.id.as_str(), "unit.identity" | "unit.restart")
        && !words.helper_ops.iter().any(|op| op == contract.id.as_str())
    {
        return Err(Availability::NotInstalled);
    }
    // A containment contract needs the whole `nft` group: a block has three phases.
    if contract.execution == ExecutionPath::Helper
        && domovoi_core::containment::ops::is_containment_contract(contract.id.as_str())
        && !domovoi_core::containment::ops::offers_nft_group(&words.helper_ops)
    {
        return Err(Availability::NotInstalled);
    }
    if contract.target_kind == TargetKind::Container && !words.container_runtime {
        return Err(Availability::Unavailable {
            reason: UnavailableReason::ContainerRuntimeUnavailable,
        });
    }
    Ok(())
}

/// Row 12 for skills: an unprivileged agent with no usable runner has nowhere to run them.
pub(super) fn runner_refusal(words: &HostWords) -> Option<UnavailableReason> {
    if words.runs_as_root {
        // A root agent still runs skills through the legacy agent process, so the runner's
        // absence is a label on the row and never a refusal of a path that works.
        return None;
    }
    match words.runner {
        RunnerState::Ready => None,
        RunnerState::Absent => Some(UnavailableReason::SkillRunnerMissing),
        RunnerState::Unsupported => Some(UnavailableReason::SkillRunnerUnsupported),
        RunnerState::Failed => Some(UnavailableReason::SkillRunnerError),
    }
}
