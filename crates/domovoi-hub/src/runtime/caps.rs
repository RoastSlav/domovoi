//! Capability objects over the hub's link (ADR 0023): the one place the composition root
//! turns the link into narrow capabilities, so no subsystem downstream of here holds the
//! raw link. These objects only choose which operation a subsystem may ask for at all.

use async_trait::async_trait;
use std::sync::Arc;

use domovoi_core::attest::Nonce32;
use domovoi_core::capability::{CatalogPageData, CatalogQuery};
use domovoi_core::containment::{ContainDispatch, ContainOutputs};
use domovoi_core::ports::AgentLink;
use domovoi_core::ports::{
    ActionAnswers, ActionCaps, AgentSnapshot, AttestationReply, AvailabilityProbe, CheckRecheck,
    CheckRerun, ContainCaps, ContainRelay, EvidenceCaps, HandoffRunner, HelperRelay, HelperReport,
    HostAvailability, HostObservation, HostRead, HostView, JournalStatus, LinkError, PackageCaps,
    PackageRelay, Presence, SkillDispatch, SkillHandoff, SkillOutput, ToolOutput, UnitCaps,
    UnitRelay,
};
use domovoi_core::unit::{UnitCall, UnitDispatch, UnitName, UnitOutputs};
use domovoi_core::{
    AgentSessionId, AuthorizationKind, CapabilityHandoff, CheckKey, ExecutionId, HostId,
    JournalEpoch, PackageDispatch, PackageOutputs, PlanHash, ReadToolCall, SkillSummary, StepId,
    TargetKey, TransferAnswer, TransferFrame,
};
use jiff::Timestamp;

use crate::capability::HostCapabilities;

struct ProbeOf {
    link: Arc<dyn AgentLink>,
}

struct ObserverOf {
    link: Arc<dyn AgentLink>,
}

struct ReaderOf {
    link: Arc<dyn AgentLink>,
}

struct HandoffOf {
    link: Arc<dyn AgentLink>,
}

struct JournalOf {
    link: Arc<dyn AgentLink>,
}

struct RerunOf {
    link: Arc<dyn AgentLink>,
}

struct RelayOf {
    link: Arc<dyn AgentLink>,
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
        ids: Vec<ExecutionId>,
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

#[async_trait]
impl PackageRelay for RelayOf {
    async fn relay_transfer(
        &self,
        host: HostId,
        frame: TransferFrame,
        expected_session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError> {
        self.link
            .send_transfer(host, frame, expected_session, deadline)
            .await
    }

    async fn dispatch_package(
        &self,
        host: HostId,
        req: PackageDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.link
            .start_capability(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn package_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError> {
        self.link.capability_result(host, handoff, deadline).await
    }
}

/// The capabilities the action services need, all over one policy link.
pub fn action_caps(link: Arc<dyn AgentLink>) -> ActionCaps {
    ActionCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        journal: Arc::new(JournalOf { link: link.clone() }),
        handoff: Arc::new(HandoffOf { link: link.clone() }),
        rerun: Arc::new(RerunOf { link }),
    }
}

/// The capabilities evidence collection needs, all over one policy link.
pub fn evidence_caps(link: Arc<dyn AgentLink>) -> EvidenceCaps {
    EvidenceCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        read: Arc::new(ReaderOf { link }),
    }
}

/// The capabilities package installation needs, all over the one link: the same probe,
/// observer and journal objects the action services hold, and the relay (ADR 0029).
pub fn package_caps(link: Arc<dyn AgentLink>) -> PackageCaps {
    PackageCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        journal: Arc::new(JournalOf { link: link.clone() }),
        relay: Arc::new(RelayOf { link }),
    }
}

struct UnitRelayOf {
    link: Arc<dyn AgentLink>,
}

#[async_trait]
impl UnitRelay for UnitRelayOf {
    async fn read_identity(
        &self,
        host: HostId,
        name: &UnitName,
        session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<UnitOutputs, LinkError> {
        // A read has no approval: it is dispatched to the live session and journal epoch under a
        // fresh execution id, labelled human, and bound to nothing.
        let epoch = self
            .link
            .journal_epoch(host)
            .ok_or(LinkError::NotConnected)?;
        // A read binds nothing, so its plan hash is a fixed placeholder the agent only journals.
        let plan_hash: PlanHash = "00".repeat(32).parse().map_err(|_| {
            LinkError::Rejected("the read's plan placeholder is invalid".to_owned())
        })?;
        let request = UnitDispatch {
            execution: ExecutionId::from_step(StepId::new()),
            call: UnitCall::Identity { name: name.clone() },
            plan_hash,
            authorization_kind: AuthorizationKind::Human,
            expires_at: deadline,
        };
        let handoff = self
            .link
            .start_unit_call(host, request, session, epoch, deadline)
            .await?;
        self.link.unit_call_result(host, handoff, deadline).await
    }

    async fn dispatch_restart(
        &self,
        host: HostId,
        req: UnitDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.link
            .start_unit_call(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn restart_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<UnitOutputs, LinkError> {
        self.link.unit_call_result(host, handoff, deadline).await
    }
}

/// The capabilities unit identity reads and restarts need, all over the one link.
pub fn unit_caps(link: Arc<dyn AgentLink>) -> UnitCaps {
    UnitCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        journal: Arc::new(JournalOf { link: link.clone() }),
        relay: Arc::new(UnitRelayOf { link }),
    }
}

struct HelperOf {
    link: Arc<dyn AgentLink>,
}

#[async_trait]
impl HelperRelay for HelperOf {
    async fn ask(
        &self,
        host: HostId,
        nonce: Nonce32,
        deadline: Timestamp,
    ) -> Result<AttestationReply, LinkError> {
        self.link.attest_safety(host, nonce, deadline).await
    }

    fn report(&self, host: HostId) -> Option<HelperReport> {
        self.link.helper_report(host)
    }

    fn connected(&self, host: HostId) -> bool {
        matches!(self.link.presence(host), Presence::Connected { .. })
    }
}

/// The helper trust code's capability over the one link: ask for an attestation, read what the
/// session says about its helper, and know whether the host is connected.
pub fn helper_relay(link: Arc<dyn AgentLink>) -> Arc<dyn HelperRelay> {
    Arc::new(HelperOf { link })
}

/// A single catalogue capability over the policy link, for the narrow `HostCatalog`
/// view that needs no read capability.
pub fn observer_of(link: Arc<dyn AgentLink>) -> Arc<dyn HostObservation> {
    Arc::new(ObserverOf { link })
}

/// The raw link as the capability registry reads it: availability, the announced catalogue
/// hash, catalogue pages, the 1.2 status words and the announced skills.
///
/// The registry holds this view rather than the link, so a subsystem that answers "is this
/// available here" cannot reach a tool, a skill dispatch or an approval (EXEC-4).
pub struct LinkCapabilities {
    link: Arc<dyn AgentLink>,
}

impl LinkCapabilities {
    /// Wraps `link` as the registry's read-only view of one host's agent.
    #[must_use]
    pub fn new(link: Arc<dyn AgentLink>) -> Self {
        Self { link }
    }
}

#[async_trait]
impl HostCapabilities for LinkCapabilities {
    async fn availability(&self, host: HostId) -> HostAvailability {
        self.link.availability(host).await
    }

    fn catalog_hash(&self, host: HostId) -> Option<String> {
        self.link.catalog_hash(host)
    }

    async fn catalog_page(
        &self,
        host: HostId,
        query: CatalogQuery,
        deadline: Timestamp,
    ) -> Result<CatalogPageData, LinkError> {
        self.link.get_catalog(host, query, deadline).await
    }

    fn view(&self, host: HostId) -> Option<HostView> {
        self.link.host_view(host)
    }
}

struct ContainRelayOf {
    link: Arc<dyn AgentLink>,
}

#[async_trait]
impl ContainRelay for ContainRelayOf {
    async fn dispatch_phase(
        &self,
        host: HostId,
        req: ContainDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.link
            .start_contain(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn phase_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<ContainOutputs, LinkError> {
        self.link.contain_result(host, handoff, deadline).await
    }
}

/// The capabilities containment needs, all over the one link.
pub fn contain_caps(link: Arc<dyn AgentLink>) -> ContainCaps {
    ContainCaps {
        probe: Arc::new(ProbeOf { link: link.clone() }),
        observer: Arc::new(ObserverOf { link: link.clone() }),
        journal: Arc::new(JournalOf { link: link.clone() }),
        relay: Arc::new(ContainRelayOf { link }),
    }
}

/// The capability registry is the one place the compiled contracts, the agent's catalogue, hub
/// enablement, the skill floors and the ceiling meet. The engine gets it beside the narrow
/// capabilities, never the link itself (ADR 0023).
pub(super) async fn registry(
    link: &Arc<dyn AgentLink>,
    store: &Arc<domovoi_store::Store>,
    clock: &Arc<dyn domovoi_core::ports::Clock>,
) -> Result<Arc<crate::capability::HubRegistry>, super::RuntimeError> {
    Ok(Arc::new(
        crate::capability::HubRegistry::new(crate::capability::RegistryDeps {
            contracts: Arc::new(
                domovoi_core::capability::ContractIndex::new(
                    domovoi_core::capability::known_contracts(),
                )
                .map_err(crate::capability::RegistryError::Index)?,
            ),
            agent: Arc::new(LinkCapabilities::new(link.clone())),
            hosts: store.clone(),
            store: store.clone(),
            clock: clock.clone(),
        })
        .await?,
    ))
}
