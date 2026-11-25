//! A fake agent link, and the recording doubles next to it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::capability::{CatalogPageData, CatalogQuery};
use domovoi_core::ports::{
    ActionAnswer, ActionAnswerEntry, ActionAnswers, AgentLink, AgentSnapshot, CheckRecheck,
    HostAvailability, HostView, HostWords, LinkError, Presence, SkillDispatch, SkillHandoff,
    SkillOutput, ToolOutput,
};
use domovoi_core::{
    ActionId, AgentId, AgentSessionId, CapabilityHandoff, CheckKey, HostId, JournalEpoch,
    PackageDispatch, PackageOutputs, ReadToolCall, SkillSummary, TargetKey, TransferAnswer,
    TransferFrame,
};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use jiff::Timestamp;

mod agent;
mod any_caps;
mod attest;
mod caps;
mod catalog;
mod holds;
mod package_link;
mod recorders;
mod request;
mod script;

pub use any_caps::{action_caps_of, evidence_caps_of, observer_of_any};
pub use attest::AttestHandler;
pub use caps::{action_caps, evidence_caps, observer_of, package_caps, unit_caps};
pub use package_link::{CapabilityHandler, CapabilityHook, PackageEvent};
pub use recorders::{RecordingReconciler, RecordingSignalSink};
pub use request::LinkRequest;

use crate::link::catalog::Catalogue;
use crate::link::holds::AvailabilityHold;
use crate::lock;

type ToolHandler =
    Box<dyn Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync>;
type SkillHandler =
    Box<dyn Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError> + Send + Sync>;
type CheckHandler = Box<
    dyn Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError> + Send + Sync,
>;
type StatusHandler = Arc<
    dyn Fn(HostId, &[ActionId], JournalEpoch) -> Result<ActionAnswers, LinkError> + Send + Sync,
>;

#[derive(Default)]
struct Handlers {
    tool: Option<ToolHandler>,
    skill: Option<SkillHandler>,
    check: Option<CheckHandler>,
}

/// An [`AgentLink`] with scripted presence, skills, host facts and handlers.
///
/// Every request is recorded. A host that is not [`Presence::Connected`] gets
/// [`LinkError::NotConnected`]; a connected host with no handler for the request kind gets
/// [`LinkError::Rejected`]. Deadlines are recorded but never enforced.
pub struct FakeAgentLink {
    presence: Mutex<BTreeMap<HostId, Presence>>,
    availability: Mutex<BTreeMap<HostId, HostAvailability>>,
    skills: Mutex<BTreeMap<HostId, Vec<SkillSummary>>>,
    facts: Mutex<BTreeMap<HostId, HostFacts>>,
    /// The hello status words the hosts announced, by host.
    words: Mutex<BTreeMap<HostId, HostWords>>,
    /// The catalogue one link serves: hashes, pages and the calls it saw.
    catalogue: Catalogue,
    checks: Mutex<BTreeMap<HostId, Vec<CheckInfo>>>,
    epochs: Mutex<BTreeMap<HostId, JournalEpoch>>,
    agents: Mutex<BTreeMap<HostId, AgentId>>,
    sessions: Mutex<BTreeMap<HostId, AgentSessionId>>,
    answers: Mutex<BTreeMap<HostId, BTreeMap<ActionId, ActionAnswer>>>,
    status: Mutex<Option<StatusHandler>>,
    /// When set, `start_skill` waits for a permit before it reports the request handed over,
    /// so a test can hold a dispatch at the point where it has been committed but not sent.
    handoff_gate: Mutex<Option<std::sync::Arc<tokio::sync::Semaphore>>>,
    /// The error `start_skill` answers with once, instead of handing the request over.
    handoff_failure: Mutex<Option<LinkError>>,
    /// When set for a host, `action_status` waits for a permit from the gate before it
    /// answers, so a test can hold a pass inside its journal query.
    status_gates: Mutex<BTreeMap<HostId, std::sync::Arc<tokio::sync::Semaphore>>>,
    /// When set for a host, `availability` waits for a permit from the gate before it
    /// answers, so a test can hold a read inside the availability check. The paired
    /// notifier fires once the check has been entered, so a test can move its fake clock
    /// only after the check is actually pending.
    avail_gates: Mutex<BTreeMap<HostId, AvailabilityHold>>,
    /// Skill requests already handed over, by request id, waiting for `skill_result`.
    sent_skills: Mutex<BTreeMap<domovoi_core::RequestId, SkillDispatch>>,
    /// Scripted transfers and package capabilities, and the ordered record of both.
    package: package_link::PackageState,
    /// Scripted helper attestation answers and the helper facts sessions announced.
    attest: attest::AttestState,
    handlers: Mutex<Handlers>,
    requests: Mutex<Vec<LinkRequest>>,
    snapshot_calls: std::sync::atomic::AtomicUsize,
    /// Test support: answers the next `snapshot` calls in order, before the live state.
    scripted_snapshots: Mutex<VecDeque<Option<AgentSnapshot>>>,
}

impl FakeAgentLink {
    /// A link where no host is connected, nothing announced and no handlers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            presence: Mutex::new(BTreeMap::new()),
            availability: Mutex::new(BTreeMap::new()),
            skills: Mutex::new(BTreeMap::new()),
            facts: Mutex::new(BTreeMap::new()),
            words: Mutex::new(BTreeMap::new()),
            catalogue: Catalogue::default(),
            checks: Mutex::new(BTreeMap::new()),
            epochs: Mutex::new(BTreeMap::new()),
            agents: Mutex::new(BTreeMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            answers: Mutex::new(BTreeMap::new()),
            status: Mutex::new(None),
            handoff_gate: Mutex::new(None),
            handoff_failure: Mutex::new(None),
            status_gates: Mutex::new(BTreeMap::new()),
            avail_gates: Mutex::new(BTreeMap::new()),
            sent_skills: Mutex::new(BTreeMap::new()),
            package: package_link::PackageState::default(),
            attest: attest::AttestState::default(),
            handlers: Mutex::new(Handlers::default()),
            requests: Mutex::new(Vec::new()),
            snapshot_calls: std::sync::atomic::AtomicUsize::new(0),
            scripted_snapshots: Mutex::new(VecDeque::new()),
        }
    }

    /// Makes `start_skill` wait for a permit from `gate` before reporting the hand-off, so a
    /// test can hold a dispatch between its commit and its send. Each `add_permits(1)`
    /// releases one hand-off.
    pub fn hold_skill_handoffs(&self, gate: std::sync::Arc<tokio::sync::Semaphore>) {
        *lock(&self.handoff_gate) = Some(gate);
    }

    /// Makes the next `start_skill` fail with `error` instead of handing the request over,
    /// as a full agent queue or an offline host would.
    pub fn fail_next_skill_handoff(&self, error: LinkError) {
        *lock(&self.handoff_failure) = Some(error);
    }

    /// Makes `action_status` for `host` wait for a permit from `gate` before answering, so a
    /// test can hold a reconciliation pass inside its journal query. The request is recorded
    /// before the wait, so the query is visible while it is held.
    pub fn hold_status_queries(&self, host: HostId, gate: std::sync::Arc<tokio::sync::Semaphore>) {
        lock(&self.status_gates).insert(host, gate);
    }

    /// Makes `availability` for `host` wait for a permit from `gate` before it answers,
    /// so a test can hold an evidence collection inside its availability check. Each
    /// `add_permits(1)` on the gate releases one check. The returned notifier fires once
    /// the check has been entered, so a test can act only then.
    pub fn hold_availability(
        &self,
        host: HostId,
        gate: std::sync::Arc<tokio::sync::Semaphore>,
    ) -> std::sync::Arc<tokio::sync::Notify> {
        let hold = AvailabilityHold::new(gate);
        let entered = hold.entered.clone();
        lock(&self.avail_gates).insert(host, hold);
        entered
    }

    /// Sets a host's presence.
    pub fn set_presence(&self, host: HostId, presence: Presence) {
        lock(&self.presence).insert(host, presence);
    }

    /// The enrolled agent this fake reports for `host`, creating one on first use.
    pub fn agent_id(&self, host: HostId) -> AgentId {
        *lock(&self.agents).entry(host).or_default()
    }

    /// Names the enrolled agent a host's session belongs to.
    pub fn set_agent(&self, host: HostId, agent: AgentId) {
        lock(&self.agents).insert(host, agent);
    }

    /// The connection identity this fake reports for `host`.
    pub fn session_id(&self, host: HostId) -> AgentSessionId {
        *lock(&self.sessions).entry(host).or_default()
    }

    /// Replaces the authenticated connection for `host`, as a reconnect would.
    pub fn new_session(&self, host: HostId) -> AgentSessionId {
        let session = AgentSessionId::new();
        lock(&self.sessions).insert(host, session);
        session
    }

    /// Sets what `availability` reports for a host, separate from its presence. A test uses
    /// this to say a host is unknown even though the fake knows when it was last seen.
    pub fn set_availability(&self, host: HostId, availability: HostAvailability) {
        lock(&self.availability).insert(host, availability);
    }

    /// Drops a scripted availability, so the host's presence decides again.
    pub fn clear_availability(&self, host: HostId) {
        lock(&self.availability).remove(&host);
    }

    /// Sets the skills a host announces.
    pub fn set_skills(&self, host: HostId, skills: Vec<SkillSummary>) {
        lock(&self.skills).insert(host, skills);
    }

    /// Sets the catalogue hash a host's agent announces in its handshake.
    pub fn set_catalog_hash(&self, host: HostId, hash: impl Into<String>) {
        self.catalogue.set_hash(host, hash);
    }

    /// Sets the hello status words a host's agent announces: root, runner, helper, ceiling
    /// summary and whether it has a container runtime.
    pub fn set_host_words(&self, host: HostId, words: HostWords) {
        lock(&self.words).insert(host, words);
    }

    /// Drops a host's announced catalogue hash, as an agent that announces none.
    pub fn clear_catalog_hash(&self, host: HostId) {
        self.catalogue.clear_hash(host);
    }

    /// Scripts the pages a host's agent answers `get_catalog` with, by page number.
    pub fn set_catalog(&self, host: HostId, pages: Vec<CatalogPageData>) {
        self.catalogue.set_pages(host, pages);
    }

    /// Answers every `get_catalog` with `handler`, instead of the scripted pages.
    pub fn on_catalog(
        &self,
        handler: impl Fn(HostId, &CatalogQuery, Timestamp) -> Result<CatalogPageData, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        self.catalogue.on_page(std::sync::Arc::new(handler));
    }

    /// Every catalogue page requested so far, oldest first.
    #[must_use]
    pub fn catalog_calls(&self) -> Vec<(HostId, CatalogQuery)> {
        self.catalogue.calls()
    }

    /// Sets what a host's agent said about its machine, as a handshake would.
    pub fn set_facts(&self, host: HostId, facts: HostFacts) {
        lock(&self.facts).insert(host, facts);
    }

    /// What the host's agent said about its machine, `None` while never announced.
    #[must_use]
    pub fn host_facts(&self, host: HostId) -> Option<HostFacts> {
        lock(&self.facts).get(&host).cloned()
    }

    /// Sets the checks a host's agent announced, as a handshake would.
    pub fn set_checks(&self, host: HostId, checks: Vec<CheckInfo>) {
        lock(&self.checks).insert(host, checks);
    }

    /// The checks the host's agent announced; empty while never announced.
    #[must_use]
    pub fn host_checks(&self, host: HostId) -> Vec<CheckInfo> {
        lock(&self.checks).get(&host).cloned().unwrap_or_default()
    }

    /// Answers `run_tool` with `handler`.
    pub fn on_tool(
        &self,
        handler: impl Fn(HostId, &ReadToolCall) -> Result<ToolOutput, LinkError> + Send + Sync + 'static,
    ) {
        lock(&self.handlers).tool = Some(Box::new(handler));
    }

    /// Answers `run_skill` with `handler`.
    pub fn on_skill(
        &self,
        handler: impl Fn(HostId, &SkillDispatch) -> Result<SkillOutput, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.handlers).skill = Some(Box::new(handler));
    }

    /// Answers `run_check` with `handler`.
    pub fn on_check(
        &self,
        handler: impl Fn(HostId, &CheckKey, Option<&TargetKey>) -> Result<CheckRecheck, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        lock(&self.handlers).check = Some(Box::new(handler));
    }

    /// Sets the journal epoch a host's agent reported.
    pub fn set_journal_epoch(&self, host: HostId, epoch: JournalEpoch) {
        lock(&self.epochs).insert(host, epoch);
    }

    /// Drops a host's journal epoch, so `journal_epoch` answers `None` for it.
    pub fn clear_journal_epoch(&self, host: HostId) {
        lock(&self.epochs).remove(&host);
    }

    /// Scripts what the host's journal says about one run.
    pub fn answer(&self, host: HostId, id: ActionId, answer: ActionAnswer) {
        lock(&self.answers)
            .entry(host)
            .or_default()
            .insert(id, answer);
    }

    /// Answers every `action_status` with `handler`, instead of the scripted answers.
    pub fn on_action_status(
        &self,
        handler: impl Fn(HostId, &[ActionId], JournalEpoch) -> Result<ActionAnswers, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        *lock(&self.status) = Some(Arc::new(handler));
    }

    /// Every request so far, oldest first.
    #[must_use]
    pub fn requests(&self) -> Vec<LinkRequest> {
        lock(&self.requests).clone()
    }

    fn connected(&self, host: HostId) -> Result<(), LinkError> {
        match self.presence(host) {
            Presence::Connected { .. } => Ok(()),
            Presence::Disconnected { .. } => Err(LinkError::NotConnected),
        }
    }
}

impl Default for FakeAgentLink {
    fn default() -> Self {
        Self::new()
    }
}

fn no_handler() -> LinkError {
    LinkError::Rejected("no handler registered".to_owned())
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
