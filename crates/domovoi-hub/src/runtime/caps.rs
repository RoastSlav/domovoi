//! Capability objects over the hub's policy link (ADR 0023): the one place the
//! composition root turns the link into narrow capabilities, so no subsystem
//! downstream of here holds the raw link. The policy wrapper stays in charge of
//! every request; these objects only choose which operation a subsystem may ask
//! for at all.

use async_trait::async_trait;
use std::sync::Arc;

use domovoi_core::ports::AgentLink;
use domovoi_core::ports::{
    ActionAnswers, ActionCaps, AgentSnapshot, AvailabilityProbe, CheckRecheck, CheckRerun,
    EvidenceCaps, HandoffRunner, HostAvailability, HostObservation, HostRead, JournalStatus,
    LinkError, Presence, SkillDispatch, SkillHandoff, SkillOutput, ToolOutput,
};
use domovoi_core::{
    ActionId, AgentSessionId, CheckKey, HostId, JournalEpoch, ReadToolCall, SkillSummary, TargetKey,
};
use jiff::Timestamp;

use crate::agent_server::PolicyLink;

struct ProbeOf {
    link: Arc<PolicyLink>,
}

struct ObserverOf {
    link: Arc<PolicyLink>,
}

struct ReaderOf {
    link: Arc<PolicyLink>,
}

struct HandoffOf {
    link: Arc<PolicyLink>,
}

struct JournalOf {
    link: Arc<PolicyLink>,
}

struct RerunOf {
    link: Arc<PolicyLink>,
}

#[async_trait]
impl AvailabilityProbe for ProbeOf {
    async fn check_availability(&self, host: HostId) -> HostAvailability {
        self.link.availability(host).await
    }
}

impl HostObservation for ObserverOf {
    fn observe_snapshot(&self, host: HostId) -> Option<AgentSnapshot> {
        self.link.snapshot(host)
    }

    fn observed_skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.link.skills(host)
    }

    fn observe_presence(&self, host: HostId) -> Presence {
        self.link.presence(host)
    }
}

#[async_trait]
impl HostRead for ReaderOf {
    async fn read_host(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        self.link.run_tool(host, call, deadline).await
    }
}

#[async_trait]
impl HandoffRunner for HandoffOf {
    async fn hand_off(
        &self,
        host: HostId,
        req: SkillDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError> {
        self.link
            .start_skill(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn await_handoff_result(
        &self,
        host: HostId,
        handoff: SkillHandoff,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        self.link.skill_result(host, handoff, deadline).await
    }
}

#[async_trait]
impl JournalStatus for JournalOf {
    fn epoch_of(&self, host: HostId) -> Option<JournalEpoch> {
        self.link.journal_epoch(host)
    }

    async fn status_batch(
        &self,
        host: HostId,
        ids: Vec<ActionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        self.link
            .action_status(host, ids, expected_epoch, deadline)
            .await
    }
}

#[async_trait]
impl CheckRerun for RerunOf {
    async fn rerun_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        self.link.run_check(host, check, target, deadline).await
    }
}

/// The capabilities the action services need, all over one policy link.
pub fn action_caps(policy_link: Arc<PolicyLink>) -> ActionCaps {
    ActionCaps {
        probe: Arc::new(ProbeOf {
            link: policy_link.clone(),
        }),
        observer: Arc::new(ObserverOf {
            link: policy_link.clone(),
        }),
        journal: Arc::new(JournalOf {
            link: policy_link.clone(),
        }),
        handoff: Arc::new(HandoffOf {
            link: policy_link.clone(),
        }),
        rerun: Arc::new(RerunOf { link: policy_link }),
    }
}

/// The capabilities evidence collection needs, all over one policy link.
pub fn evidence_caps(policy_link: Arc<PolicyLink>) -> EvidenceCaps {
    EvidenceCaps {
        probe: Arc::new(ProbeOf {
            link: policy_link.clone(),
        }),
        observer: Arc::new(ObserverOf {
            link: policy_link.clone(),
        }),
        read: Arc::new(ReaderOf { link: policy_link }),
    }
}

/// A single catalogue capability over the policy link, for the narrow `HostCatalog`
/// view that needs no read capability.
pub fn observer_of(link: Arc<PolicyLink>) -> Arc<dyn HostObservation> {
    Arc::new(ObserverOf { link })
}
