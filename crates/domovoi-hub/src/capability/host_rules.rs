//! Rows 11 and 12 of the availability table for the host's own words: the ceiling, the helper
//! and what it offers, and the container runtime; and where a skill would actually run.

use domovoi_core::capability::{
    Availability, CapabilityContract, EffectClass, ExecutionPath, NotAllowedOnHost, TargetKind,
    UnavailableReason, is_policy_only,
};
use domovoi_core::ports::{HelperState, HostWords, RunnerState};
use domovoi_core::{HostNeed, Risk, SkillSummary};

use super::ceiling::{CeilingParse, covers, parse_ceiling};

/// Row 11: whether the agent's ceiling covers a non-`Read` compiled contract. Skills are
/// not gated here: they run only on a person's approval.
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

/// Why the helper on this host cannot serve `contract`, when it cannot: the helper failed, or it
/// does not offer the operation (a group is missing from `helper.toml`). `None` for a contract
/// that does not run through the helper and for one the helper offers. One function, so the
/// availability rows and the Limits tab say the same thing.
pub(crate) fn helper_refusal(
    contract: &CapabilityContract,
    words: &HostWords,
) -> Option<UnavailableReason> {
    if contract.execution != ExecutionPath::Helper {
        return None;
    }
    if words.helper == HelperState::Failed {
        return Some(UnavailableReason::HelperError);
    }
    // The helper offers an operation only when it says so: a read it serves, or a write whose
    // group is named and whose platform tier allows it.
    if matches!(contract.id.as_str(), "unit.identity" | "unit.restart")
        && !words.helper_ops.iter().any(|op| op == contract.id.as_str())
    {
        return Some(UnavailableReason::HelperSystemdGroupMissing);
    }
    // A containment contract needs the whole `nft` group: a block has three phases.
    if domovoi_core::containment::ops::is_containment_contract(contract.id.as_str())
        && !domovoi_core::containment::ops::offers_nft_group(&words.helper_ops)
    {
        return Some(UnavailableReason::HelperNftGroupMissing);
    }
    None
}

/// Rows 11 and 12 for a compiled contract: ceiling, helper, container runtime.
pub(super) fn host_allows(
    contract: &CapabilityContract,
    words: &HostWords,
) -> Result<(), Availability> {
    // The ceiling is the limit for unattended runs. A contract a person approves needs no
    // entry: the agent checks the ceiling only for a policy permit. Only a contract that runs
    // through policy alone (containment) is refused without one.
    if contract.effect != EffectClass::Read && is_policy_only(&contract.id) {
        ceiling_allows(contract, words)?;
    }
    if let Some(reason) = helper_refusal(contract, words) {
        return Err(Availability::Unavailable { reason });
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
        RunnerState::Absent if words.in_container => Some(UnavailableReason::AgentInContainer),
        RunnerState::Absent => Some(UnavailableReason::SkillRunnerMissing),
        RunnerState::Unsupported => Some(UnavailableReason::SkillRunnerUnsupported),
        RunnerState::Failed => Some(UnavailableReason::SkillRunnerError),
    }
}

/// Why one skill cannot run where skills run on this host: no usable runner, a skill that
/// changes the host with no root to do it, or a declared need the runner sandbox hides.
pub(super) fn skill_refusal(
    words: &HostWords,
    summary: &SkillSummary,
) -> Option<UnavailableReason> {
    if let Some(reason) = runner_refusal(words) {
        return Some(reason);
    }
    if words.runs_as_root {
        return None;
    }
    // The skill runner's account is not root and has no way to switch user, so a write skill
    // would fail at run time with nothing to say why.
    if summary.risk != Risk::Read {
        return Some(UnavailableReason::WriteSkillNeedsRoot);
    }
    if let Some(need) = summary.hidden_needs.first() {
        return Some(match need {
            HostNeed::Services => UnavailableReason::SkillNeedsServices,
            HostNeed::Processes => UnavailableReason::SkillNeedsProcesses,
            HostNeed::Journal => UnavailableReason::SkillNeedsJournal,
            HostNeed::Hardware => UnavailableReason::SkillNeedsHardware,
            HostNeed::Files => UnavailableReason::SkillNeedsFiles,
        });
    }
    summary
        .agent_journal_unreadable
        .then_some(UnavailableReason::AgentJournalUnreadable)
}
