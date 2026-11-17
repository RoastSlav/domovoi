//! Capability objects over any [`AgentLink`], so a test link that is not
//! [`FakeAgentLink`] can be handed to the engine the same narrow way the hub hands its
//! transport. These forward one operation each: they exist to keep a test harness off the
//! raw trait object, not to script behaviour.

use async_trait::async_trait;
use std::sync::Arc;

use domovoi_core::ports::AgentLink;
use domovoi_core::ports::{ActionAnswers, AgentSnapshot};
use domovoi_core::ports::{
    ActionCaps, AvailabilityProbe, CheckRecheck, CheckRerun, EvidenceCaps, HandoffRunner,
    HostAvailability, HostObservation, HostRead, JournalStatus, LinkError, Presence, SkillDispatch,
    SkillHandoff, SkillOutput, ToolOutput,
};
use domovoi_core::triage::ReadToolCall;
use domovoi_core::{
    ActionId, AgentSessionId, CheckKey, HostId, JournalEpoch, SkillSummary, TargetKey,
};
use jiff::Timestamp;

struct ProbeAny {
    link: Arc<dyn AgentLink>,
}

struct ObserveAny {
    link: Arc<dyn AgentLink>,
}

struct ReadAny {
    link: Arc<dyn AgentLink>,
}

struct HandoffAny {
    link: Arc<dyn AgentLink>,
}

struct JournalAny {
    link: Arc<dyn AgentLink>,
}

struct RerunAny {
    link: Arc<dyn AgentLink>,
}

#[async_trait]
impl AvailabilityProbe for ProbeAny {
    async fn check_availability(&self, host: HostId) -> HostAvailability {
        self.link.availability(host).await
    }
}

impl HostObservation for ObserveAny {
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
impl HostRead for ReadAny {
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
impl HandoffRunner for HandoffAny {
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
impl JournalStatus for JournalAny {
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
impl CheckRerun for RerunAny {
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

/// The action capabilities over a scripted link of any implementation.
pub fn action_caps_of(link: Arc<dyn AgentLink>) -> ActionCaps {
    ActionCaps {
        probe: Arc::new(ProbeAny { link: link.clone() }),
        observer: Arc::new(ObserveAny { link: link.clone() }),
        journal: Arc::new(JournalAny { link: link.clone() }),
        handoff: Arc::new(HandoffAny { link: link.clone() }),
        rerun: Arc::new(RerunAny { link }),
    }
}

/// The evidence capabilities over a scripted link of any implementation.
pub fn evidence_caps_of(link: Arc<dyn AgentLink>) -> EvidenceCaps {
    EvidenceCaps {
        probe: Arc::new(ProbeAny { link: link.clone() }),
        observer: Arc::new(ObserveAny { link: link.clone() }),
        read: Arc::new(ReadAny { link }),
    }
}

/// A single catalogue capability over a scripted link of any implementation.
pub fn observer_of_any(link: Arc<dyn AgentLink>) -> Arc<dyn HostObservation> {
    Arc::new(ObserveAny { link })
}
