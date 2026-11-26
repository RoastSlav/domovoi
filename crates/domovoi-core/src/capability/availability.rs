//! Whether the hub can offer one contract on one host, and why not.
//!
//! [`Availability`] answers "would the hub offer this capability here", which is a different
//! question from "may this request run unattended" (the policy evaluator owns that one). An
//! `Available` answer never means approved: it says the host implements it, the hub enables it
//! and nothing the host needs is missing. A refusal here is only ever about a missing piece,
//! never about authorization.

use serde::{Deserialize, Serialize};

use super::catalog::CatalogueState;
use super::contract::CapabilityContract;
use super::effect::{EffectClass, ExecutionPath, TargetKind};
use super::ids::{ContractCategory, ContractId, ContractVersion};
use jiff::Timestamp;

/// Whether one contract is offered on one host, and why it is not.
///
/// The eight states are the whole answer the hub can give from the agent's own report plus
/// hub state; none of them is an authorization decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Availability {
    /// The hub would offer it here: implemented, enabled, allowed by the host's own rules.
    Available,
    /// The host's complete implementation list does not contain it.
    NotInstalled,
    /// The owner switched it off in the hub; the agent still has it.
    DisabledByHub,
    /// The host's own configuration refuses this class of capability.
    NotAllowedOnHost {
        /// Which of the host's rules refuses it.
        reason: NotAllowedOnHost,
    },
    /// The host implements it, but something it needs is missing or broken now.
    Unavailable {
        /// What is missing.
        reason: UnavailableReason,
    },
    /// The host has had no agent session in this hub run.
    HostUnknown,
    /// The host had a session in this hub run and it ended.
    HostAbsent {
        /// When the last session ended.
        since: Timestamp,
    },
    /// The hub refused this host's agent: it speaks another protocol version.
    HostIncompatible {
        /// The version the agent reported.
        reported: String,
    },
}

/// Why the host's own configuration refuses a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotAllowedOnHost {
    /// No ceiling summary was reported, or the reported ceiling is disabled.
    CeilingAbsent,
    /// A ceiling exists but has no entry for this exact contract id.
    CeilingEntryMissing,
    /// The agent's local risk allowlist does not cover the effect class. Defined for the
    /// wire field that does not exist yet; never produced in this milestone.
    AllowRisk,
}

/// Why a host cannot attempt a contract it does implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The agent is unprivileged and reports no skill runner: skills have nowhere to run.
    SkillRunnerMissing,
    /// The agent reported the skill runner as unsupported.
    SkillRunnerUnsupported,
    /// The agent reported the skill runner as failing.
    SkillRunnerError,
    /// A helper-path contract on a host whose agent reports no helper.
    HelperMissing,
    /// The agent reported the helper as failing.
    HelperError,
    /// The catalogue walk did not finish: a page failed, the host disconnected, the walk
    /// was cut by its deadline, or no hash was announced.
    CatalogueIncomplete,
    /// The agent announced a catalogue hash the pages do not match.
    CatalogueHashMismatch,
    /// The catalogue belongs to a hash the agent no longer announces.
    CatalogueStale,
    /// A complete catalogue and `hello.skills` do not name the same skill set.
    CatalogueInconsistent,
    /// A container-targeted contract on a host whose agent reports no usable container
    /// runtime.
    ContainerRuntimeUnavailable,
    /// `container.restart`: Docker write access needs its own helper decision and has no
    /// dispatch path, for either authorization kind.
    DockerWriteUnsupported,
    /// The agent runs inside a container, so it cannot reach the host's helper or skill
    /// runner. The true cause of a refusal that would otherwise read as a missing helper
    /// or runner.
    AgentInContainer,
}

impl Availability {
    /// Every state the hub can answer with, each carrying a payload value its data-carrying
    /// variants need. A function rather than a constant array because one
    /// state carries a timestamp and another a string; the UI vocabulary test walks it.
    #[must_use]
    pub fn all() -> Vec<Availability> {
        let since = Timestamp::constant(1_700_000_000, 0);
        vec![
            Self::Available,
            Self::NotInstalled,
            Self::DisabledByHub,
            Self::NotAllowedOnHost {
                reason: NotAllowedOnHost::CeilingAbsent,
            },
            Self::Unavailable {
                reason: UnavailableReason::CatalogueIncomplete,
            },
            Self::HostUnknown,
            Self::HostAbsent { since },
            Self::HostIncompatible {
                reported: "1.1".to_owned(),
            },
        ]
    }

    /// The stable wire and API code of this state.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::NotInstalled => "not_installed",
            Self::DisabledByHub => "disabled_by_hub",
            Self::NotAllowedOnHost { .. } => "not_allowed_on_host",
            Self::Unavailable { .. } => "unavailable",
            Self::HostUnknown => "host_unknown",
            Self::HostAbsent { .. } => "host_absent",
            Self::HostIncompatible { .. } => "host_incompatible",
        }
    }

    /// The reason code of a refused state, when the state has one.
    #[must_use]
    pub fn reason_code(&self) -> Option<&'static str> {
        match self {
            Self::NotAllowedOnHost { reason } => Some(reason.code()),
            Self::Unavailable { reason } => Some(reason.code()),
            _ => None,
        }
    }

    /// A short owner-facing sentence about this state. Never a raw code.
    #[must_use]
    pub fn describe(&self) -> String {
        let sentence: &'static str = match self {
            Self::Available => "available on this host",
            Self::NotInstalled => "this host's agent does not implement it",
            Self::DisabledByHub => "disabled for this host in the hub",
            Self::NotAllowedOnHost { reason } => reason.describe(),
            Self::Unavailable { reason } => reason.describe(),
            Self::HostUnknown => "this host has never connected to this hub",
            Self::HostAbsent { .. } => "this host's agent is offline",
            Self::HostIncompatible { .. } => {
                "this host's agent speaks another protocol version; upgrade it"
            }
        };
        sentence.to_string()
    }

    /// Whether the hub would offer the capability here.
    #[must_use]
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

impl NotAllowedOnHost {
    /// The wire code of this reason.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::CeilingAbsent => "ceiling_absent",
            Self::CeilingEntryMissing => "ceiling_entry_missing",
            Self::AllowRisk => "allow_risk",
        }
    }

    /// A short owner-facing sentence about this reason.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Self::CeilingAbsent => "this host states no unattended limit for this class",
            Self::CeilingEntryMissing => "this host has no unattended limit for this contract",
            Self::AllowRisk => "this host's risk allowlist does not cover this class",
        }
    }
}

impl UnavailableReason {
    /// The wire code of this reason.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::SkillRunnerMissing => "skill_runner_missing",
            Self::SkillRunnerUnsupported => "skill_runner_unsupported",
            Self::SkillRunnerError => "skill_runner_error",
            Self::HelperMissing => "helper_missing",
            Self::HelperError => "helper_error",
            Self::CatalogueIncomplete => "catalogue_incomplete",
            Self::CatalogueHashMismatch => "catalogue_hash_mismatch",
            Self::CatalogueStale => "catalogue_stale",
            Self::CatalogueInconsistent => "catalogue_inconsistent",
            Self::ContainerRuntimeUnavailable => "container_runtime_unavailable",
            Self::DockerWriteUnsupported => "docker_write_unsupported",
            Self::AgentInContainer => "agent_in_container",
        }
    }

    /// A short owner-facing sentence about this reason.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Self::SkillRunnerMissing => "this host's agent has no skill runner to run skills in",
            Self::SkillRunnerUnsupported => "the skill runner on this host cannot be used",
            Self::SkillRunnerError => "the skill runner on this host reported an error",
            Self::HelperMissing => "this host has no helper for privileged operations",
            Self::HelperError => "the helper on this host reported an error",
            Self::CatalogueIncomplete => "this host's capability catalogue is not verified",
            Self::CatalogueHashMismatch => {
                "the catalogue this host serves does not match what it announced"
            }
            Self::CatalogueStale => "the catalogue read for this host is out of date",
            Self::CatalogueInconsistent => "the agent's skill list and its catalogue disagree",
            Self::ContainerRuntimeUnavailable => "this host has no usable container runtime",
            Self::DockerWriteUnsupported => {
                "Docker write access needs its own helper decision and is not available"
            }
            Self::AgentInContainer => {
                "this agent runs in a container and cannot reach the host's helper or skill runner"
            }
        }
    }
}

/// One row of the capability inventory: a contract as one host sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityEntry {
    /// The contract this row is about.
    pub contract: ContractId,
    /// The contract's version.
    pub version: ContractVersion,
    /// The contract's human-facing title.
    pub title: String,
    /// The family the contract belongs to.
    pub category: ContractCategory,
    /// What the contract can do, after any hub floor raised it.
    pub effect: EffectClass,
    /// What kind of thing the contract acts on.
    pub target_kind: TargetKind,
    /// Where the contract is designed to execute.
    pub execution: ExecutionPath,
    /// Whether the hub offers it on this host.
    pub state: Availability,
    /// How much of this host's implementation list the hub has read.
    pub catalogue: CatalogueState,
    /// `hello.runs_as_root`: a label about the agent, never a refusal.
    pub root_agent: bool,
    /// `hello.in_container`: a label about the agent, never a refusal by itself.
    pub in_container: bool,
    /// The isolated skill runner exists on this host, whatever this contract's path is.
    pub runner_installed: bool,
    /// The implementation came from `hello.skills` rather than a catalogue page.
    pub from_hello_skills: bool,
}

impl AvailabilityEntry {
    /// The row of `contract` as one host sees it, with the host's own status labels.
    #[must_use]
    pub fn new(
        contract: &CapabilityContract,
        state: Availability,
        catalogue: CatalogueState,
        root_agent: bool,
        in_container: bool,
        runner_installed: bool,
        from_hello_skills: bool,
    ) -> Self {
        Self {
            contract: contract.id.clone(),
            version: contract.version,
            title: contract.title.clone(),
            category: contract.category.clone(),
            effect: contract.effect,
            target_kind: contract.target_kind,
            execution: contract.execution,
            state,
            catalogue,
            root_agent,
            in_container,
            runner_installed,
            from_hello_skills,
        }
    }
}
