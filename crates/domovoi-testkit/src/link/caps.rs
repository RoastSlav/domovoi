//! Capability objects over a fake link: the narrow views a test hands to the
//! engine's dependency structures (ADR 0023). Each object reaches the scripted
//! fake through one capability, so a test cannot widen one object into another.

use async_trait::async_trait;
use std::sync::Arc;

use domovoi_core::ports::AgentLink;
use domovoi_core::ports::{ActionAnswers, AgentSnapshot, HostAvailability, SkillDispatch};
use domovoi_core::ports::{
    ActionCaps, AvailabilityProbe, CheckRecheck, CheckRerun, EvidenceCaps, HandoffRunner,
    HostObservation, HostRead, JournalStatus, LinkError, Presence, SkillHandoff, SkillOutput,
    ToolOutput,
};
use domovoi_core::triage::ReadToolCall;
use domovoi_core::{
    ActionId, AgentSessionId, CheckKey, HostId, JournalEpoch, SkillSummary, TargetKey,
};
use jiff::Timestamp;

use super::FakeAgentLink;

struct ProbeOf {
    link: Arc<FakeAgentLink>,
}

struct ObserverOf {
    link: Arc<FakeAgentLink>,
}

struct ReaderOf {
    link: Arc<FakeAgentLink>,
}

struct HandoffOf {
    link: Arc<FakeAgentLink>,
}

struct JournalOf {
    link: Arc<FakeAgentLink>,
}

struct RerunOf {
    link: Arc<FakeAgentLink>,
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

/// The capabilities the action services need, all over one scripted fake.
pub fn action_caps(link: Arc<FakeAgentLink>) -> ActionCaps {
    ActionCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        journal: Arc::new(JournalOf { link: link.clone() }),
        handoff: Arc::new(HandoffOf { link: link.clone() }),
        rerun: Arc::new(RerunOf { link }),
    }
}

/// The capabilities evidence collection needs, all over one scripted fake.
pub fn evidence_caps(link: Arc<FakeAgentLink>) -> EvidenceCaps {
    EvidenceCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        read: Arc::new(ReaderOf { link }),
    }
}

/// A single catalogue capability over the scripted fake, for the narrow `HostCatalog`
/// view that needs no read capability.
pub fn observer_of(link: Arc<FakeAgentLink>) -> Arc<dyn HostObservation> {
    Arc::new(ObserverOf { link })
}
