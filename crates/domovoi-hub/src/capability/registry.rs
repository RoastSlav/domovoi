//! The hub's capability registry: one host's contracts, combined into one answer per row.
//!
//! This is the only place that combines the compiled contract index, the walked agent
//! catalogue, `hello.skills`, the host's own status words, hub enablement, the raise-only
//! skill floors and the agent's ceiling summary. The precedence between them is fixed by
//! the order in [`HubRegistry::index_state`] and [`HubRegistry::skill_state`], and must not
//! be reordered, because a later answer would then rest on a weaker fact.
//!
//! The registry never evaluates policy, approves anything or dispatches: `Available` means
//! "the hub would offer it here" and never "approved" (RW-10). Enforcement stays at the two
//! engine boundaries, which ask this registry whether a contract is enabled.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::{
    Availability, AvailabilityEntry, CapabilityContract, CatalogSnapshot, CatalogueGap,
    CatalogueState, ContractId, ContractIndex, EffectClass, ExecutionPath, IndexError,
    NotAllowedOnHost, TargetKind, UnavailableReason, effective_effect, skill_contract,
};
use domovoi_core::ports::{
    Clock, HelperState, HostAvailability, HostRepo, HostView, HostWords, RunnerState, SettingsRepo,
    StoreError,
};
use domovoi_core::{Host, HostId, SkillName, SkillSummary};
use jiff::Timestamp;

use super::catalogue::CatalogueCache;
use super::ceiling::{CeilingParse, covers, parse_ceiling};
use super::enablement::Enablement;
use super::floors::{FloorWriteError, Floors};
use super::host::HostCapabilities;

/// The pieces the registry is built from, in the order they can be built without a cycle.
pub struct RegistryDeps {
    /// The hub's compiled contracts.
    pub contracts: Arc<ContractIndex>,
    /// The agents' live sessions.
    pub agent: Arc<dyn HostCapabilities>,
    /// Enrolled hosts and their agents.
    pub hosts: Arc<dyn HostRepo>,
    /// The settings store holding enablement and floors.
    pub store: Arc<dyn SettingsRepo>,
    /// The hub's clock, for walk deadlines and snapshot times.
    pub clock: Arc<dyn Clock>,
}

/// Why the registry could not be built.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The compiled contract set is not a valid index.
    #[error(transparent)]
    Index(#[from] IndexError),
    /// The settings store could not be read.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// The hub's answer to "which contracts can this host run, and why not".
pub struct HubRegistry {
    contracts: Arc<ContractIndex>,
    agent: Arc<dyn HostCapabilities>,
    hosts: Arc<dyn HostRepo>,
    clock: Arc<dyn Clock>,
    enablement: Arc<Enablement>,
    floors: Arc<Floors>,
    catalogue: Arc<CatalogueCache>,
}

impl HubRegistry {
    /// Validates the index once and loads the settings-backed state for every host.
    pub async fn new(deps: RegistryDeps) -> Result<Self, RegistryError> {
        // The set was validated where it was built; validating it again here is what makes
        // this the one place a broken set is refused before anything reads it.
        let contracts = Arc::new(ContractIndex::new(
            deps.contracts.iter().cloned().collect::<Vec<_>>(),
        )?);
        let hosts = deps.hosts.list_hosts().await?;
        let enablement = Arc::new(Enablement::load(deps.store.clone(), &hosts).await?);
        let floors = Arc::new(Floors::load(deps.store, &hosts).await?);
        let catalogue = Arc::new(CatalogueCache::new(
            deps.agent.clone(),
            deps.clock.clone(),
            contracts.clone(),
        ));
        Ok(Self {
            contracts,
            agent: deps.agent,
            hosts: deps.hosts,
            clock: deps.clock,
            enablement,
            floors,
            catalogue,
        })
    }

    /// The enrolled host with `id`, for a caller holding only the id.
    pub async fn host(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        self.hosts.get_host(id).await
    }

    /// One host's whole inventory, already computed. Used by the API.
    ///
    /// The catalogue is walked once for the whole list, never once per row, and no lock is
    /// held while it is walked.
    pub async fn inventory(&self, host: HostId) -> Vec<AvailabilityEntry> {
        let seen = self.seen(host).await;
        let mut rows = self
            .contracts
            .iter()
            .map(|contract| self.index_row(contract, &seen))
            .collect::<Vec<_>>();
        rows.extend(
            seen.skills
                .values()
                .map(|summary| self.skill_row(host, summary, &seen)),
        );
        rows.sort_by(|left, right| left.contract.cmp(&right.contract));
        rows
    }

    /// Contracts the agent reported that the hub does not know. Diagnostics only.
    ///
    /// A `skill.<name>` echo is only unknown when `hello.skills` does not describe the same
    /// name; otherwise the row exists and the echo is its integrity check.
    pub fn unknown_contracts(&self, host: HostId) -> Vec<String> {
        let Some(snapshot) = self.catalogue.cached(host) else {
            return Vec::new();
        };
        let announced = self.agent.view(host).map(|view| view_skills(&view));
        let mut unknown = snapshot.unknown.clone();
        for id in snapshot.entries.keys() {
            let Some(name) = id.skill_name() else {
                continue;
            };
            let described = announced
                .as_ref()
                .is_some_and(|skills| skills.contains_key(&name));
            if !described {
                unknown.push(id.to_string());
            }
        }
        unknown.sort();
        unknown
    }

    /// The floor write path: raise one skill's class on `host`, or clear the floor.
    pub async fn set_floor(
        &self,
        host: &Host,
        skill: SkillName,
        floor: Option<EffectClass>,
    ) -> Result<(), FloorWriteError> {
        let declared = self.declared_of(host.id, &skill);
        self.floors.set(host, skill, declared, floor).await
    }

    /// Turns one skill on or off for `host`, in memory and in the settings store.
    pub async fn set_skill_enabled(
        &self,
        host: &Host,
        skill: SkillName,
        enabled: bool,
    ) -> Result<(), StoreError> {
        self.enablement.set(host, skill, enabled).await
    }

    /// The host's state as one read: availability, the session's words and skills, and the
    /// catalogue.
    ///
    /// The words and skills come from one `view`, so a reconnect cannot mix one session's
    /// status with another session's skills. A session that ends between the availability
    /// read and the view is asked about again, and a link that still reports a session it
    /// cannot describe yields no facts at all: a missing read is never read as an explicit
    /// report of absence.
    async fn seen(&self, host: HostId) -> Seen {
        let mut availability = self.agent.availability(host).await;
        let mut view = self.view_of(&availability, host);
        if matches!(availability, HostAvailability::Known { .. }) && view.is_none() {
            availability = self.agent.availability(host).await;
            if matches!(availability, HostAvailability::Known { .. }) {
                view = self.view_of(&availability, host);
            }
        }
        let facts = view.is_some();
        let words = view
            .as_ref()
            .map(|view| view.words.clone())
            .unwrap_or_default();
        let skills = view.as_ref().map(view_skills).unwrap_or_default();
        let catalogue = if facts && matches!(availability, HostAvailability::Known { .. }) {
            self.catalogue.catalog(host).await
        } else {
            unread(host, self.clock.now())
        };
        Seen {
            availability,
            words,
            skills,
            facts,
            catalogue,
        }
    }

    /// The session's words and skills, when the host is connected now.
    fn view_of(&self, availability: &HostAvailability, host: HostId) -> Option<HostView> {
        match availability {
            HostAvailability::Known { .. } => self.agent.view(host),
            _ => None,
        }
    }

    /// The class `skill` declares on `host`, from the agent's own summary.
    fn declared_of(&self, host: HostId, skill: &SkillName) -> EffectClass {
        self.agent
            .view(host)
            .and_then(|view| {
                view.skills
                    .into_iter()
                    .find(|summary| &summary.name == skill)
            })
            .map_or(EffectClass::Destructive, |summary| {
                EffectClass::from(summary.risk)
            })
    }

    /// One row of a contract the hub compiled in.
    fn index_row(&self, contract: &CapabilityContract, seen: &Seen) -> AvailabilityEntry {
        let state = self.index_state(contract, seen);
        entry_of(contract, state, seen)
    }

    /// One row of a skill, whose description and contract come from `hello.skills`.
    fn skill_row(&self, host: HostId, summary: &SkillSummary, seen: &Seen) -> AvailabilityEntry {
        let built = skill_contract(summary, self.floors.get(host, &summary.name));
        let state = self.skill_availability(host, &built.id, seen);
        entry_of(&built, state, seen)
    }

    /// Rows 1-3, then rows 5-7, 10-13: the state of a compiled contract.
    fn index_state(&self, contract: &CapabilityContract, seen: &Seen) -> Availability {
        if let Some(state) = host_state(seen) {
            return state;
        }
        if contract.execution == ExecutionPath::Agent {
            if let Some(reason) = catalogue_refusal(&seen.catalogue.state) {
                return Availability::Unavailable { reason };
            }
            if !seen.catalogue.entries.contains_key(&contract.id) {
                return Availability::NotInstalled;
            }
        }
        if contract.execution == ExecutionPath::Helper && seen.words.helper == HelperState::Absent {
            return Availability::Unavailable {
                reason: UnavailableReason::HelperMissing,
            };
        }
        match host_allows(contract, &seen.words) {
            Ok(()) => Availability::Available,
            Err(state) => state,
        }
    }

    /// Rows 1-3, then 4, 8, 9, 11 and 12: the state of the `skill.<name>` family.
    fn skill_availability(&self, host: HostId, id: &ContractId, seen: &Seen) -> Availability {
        if let Some(state) = host_state(seen) {
            return state;
        }
        let Some(name) = id.skill_name() else {
            // A `skill.` id that is not a skill name describes nothing the hub can offer.
            return Availability::NotInstalled;
        };
        // Row 4: the owner's switch outranks anything the host says.
        if self.enablement.is_disabled(host, &name) {
            return Availability::DisabledByHub;
        }
        // Row 8: no summary means no contract; an id alone never creates one.
        let Some(summary) = seen.skills.get(&name) else {
            return Availability::NotInstalled;
        };
        let built = skill_contract(summary, self.floors.get(host, &name));
        // Row 9: a complete catalogue that does not echo the skill contradicts hello.
        if seen.catalogue.state.is_complete() && !seen.catalogue.entries.contains_key(&built.id) {
            return Availability::Unavailable {
                reason: UnavailableReason::CatalogueInconsistent,
            };
        }
        // Row 11: the host's ceiling.
        if built.effect != EffectClass::Read
            && let Err(state) = ceiling_allows(&built, &seen.words)
        {
            return state;
        }
        // Row 12: where a skill would actually run.
        match runner_refusal(&seen.words) {
            Some(reason) => Availability::Unavailable { reason },
            None => Availability::Available,
        }
    }
}

/// Everything one host read says, taken once per inventory and per availability query.
struct Seen {
    availability: HostAvailability,
    words: HostWords,
    skills: BTreeMap<SkillName, SkillSummary>,
    /// Whether the words and skills came from one session that could be read. Without it the
    /// answer proves nothing about this host, whatever the connection state says.
    facts: bool,
    catalogue: CatalogSnapshot,
}

/// The agent's announced skills, by name.
fn view_skills(view: &HostView) -> BTreeMap<SkillName, SkillSummary> {
    view.skills
        .iter()
        .map(|summary| (summary.name.clone(), summary.clone()))
        .collect()
}

/// One inventory row from a contract and the state it reached.
fn entry_of(contract: &CapabilityContract, state: Availability, seen: &Seen) -> AvailabilityEntry {
    AvailabilityEntry {
        contract: contract.id.clone(),
        version: contract.version,
        title: contract.title.clone(),
        category: contract.category.clone(),
        effect: contract.effect,
        target_kind: contract.target_kind,
        execution: contract.execution,
        state,
        catalogue: seen.catalogue.state.clone(),
        root_agent: seen.words.runs_as_root,
        runner_installed: seen.words.runner == RunnerState::Ready,
        from_hello_skills: contract.id.is_skill(),
    }
}

/// A snapshot for a host that was not read, because no walk could prove one.
fn unread(host: HostId, at: Timestamp) -> CatalogSnapshot {
    CatalogSnapshot {
        host,
        announced_hash: None,
        state: CatalogueState::Partial {
            gap: CatalogueGap::NotWalked,
        },
        entries: BTreeMap::new(),
        unknown: Vec::new(),
        pages: 0,
        session_since: None,
        at,
    }
}

/// Rows 1-3: the host itself, which outranks every other input.
fn host_state(seen: &Seen) -> Option<Availability> {
    match &seen.availability {
        // A live session whose facts could not be read proves nothing about the host: its
        // status is unknown, not an explicit report of absence, so no row may claim
        // "runner missing", "no ceiling" or "not installed" from it.
        HostAvailability::Known { .. } if !seen.facts => Some(Availability::HostUnknown),
        HostAvailability::Known { .. } => None,
        HostAvailability::Unknown => Some(Availability::HostUnknown),
        HostAvailability::Absent { since } => Some(Availability::HostAbsent { since: *since }),
        HostAvailability::Incompatible { reported } => Some(Availability::HostIncompatible {
            reported: reported.clone(),
        }),
    }
}

/// Row 5: why an incomplete catalogue makes an `Agent` contract unavailable.
fn catalogue_refusal(state: &CatalogueState) -> Option<UnavailableReason> {
    match state {
        CatalogueState::Complete { .. } => None,
        CatalogueState::Stale { .. } => Some(UnavailableReason::CatalogueStale),
        CatalogueState::Partial {
            gap: CatalogueGap::HashMismatch,
        } => Some(UnavailableReason::CatalogueHashMismatch),
        CatalogueState::Partial { .. } => Some(UnavailableReason::CatalogueIncomplete),
    }
}

/// Row 11: whether the agent's ceiling covers a non-`Read` contract.
fn ceiling_allows(contract: &CapabilityContract, words: &HostWords) -> Result<(), Availability> {
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
fn host_allows(contract: &CapabilityContract, words: &HostWords) -> Result<(), Availability> {
    if contract.effect != EffectClass::Read {
        ceiling_allows(contract, words)?;
    }
    if contract.execution == ExecutionPath::Helper && words.helper == HelperState::Failed {
        return Err(Availability::Unavailable {
            reason: UnavailableReason::HelperError,
        });
    }
    if contract.target_kind == TargetKind::Container && !words.container_runtime {
        return Err(Availability::Unavailable {
            reason: UnavailableReason::ContainerRuntimeUnavailable,
        });
    }
    Ok(())
}

/// Row 12 for skills: an unprivileged agent with no usable runner has nowhere to run them.
fn runner_refusal(words: &HostWords) -> Option<UnavailableReason> {
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

#[async_trait::async_trait]
impl domovoi_core::ports::CapabilityRegistry for HubRegistry {
    async fn availability(&self, host: HostId, contract: &ContractId) -> Availability {
        let seen = self.seen(host).await;
        if contract.is_skill() {
            return self.skill_availability(host, contract, &seen);
        }
        match self.contracts.get(contract) {
            // An id the hub did not compile in is not a contract it can offer, whatever an
            // agent's page says about it.
            None => match host_state(&seen) {
                Some(state) => state,
                None => Availability::NotInstalled,
            },
            Some(known) => self.index_state(known, &seen),
        }
    }

    async fn catalog(&self, host: HostId) -> CatalogSnapshot {
        self.catalogue.catalog(host).await
    }

    fn contracts(&self) -> &ContractIndex {
        &self.contracts
    }

    fn effective_effect(&self, host: HostId, skill: &SkillName) -> Option<EffectClass> {
        let declared = self
            .agent
            .view(host)?
            .skills
            .into_iter()
            .find(|summary| &summary.name == skill)
            .map(|summary| EffectClass::from(summary.risk))?;
        Some(effective_effect(declared, self.floors.get(host, skill)))
    }

    fn enabled(&self, host: HostId, contract: &ContractId) -> bool {
        match contract.skill_name() {
            Some(name) => !self.enablement.is_disabled(host, &name),
            // Nothing but the `skill.<name>` family has an owner switch in this milestone.
            None => true,
        }
    }

    fn enabled_skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.agent
            .view(host)
            .map(|view| view.skills)
            .unwrap_or_default()
            .into_iter()
            .filter(|summary| !self.enablement.is_disabled(host, &summary.name))
            .collect()
    }
}
