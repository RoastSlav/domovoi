//! A fake capability registry, so the engine's boundaries can be exercised without a hub.
//!
//! It answers exactly what a test scripts: the host's skills, whether a `(host, contract)`
//! pair is available, and the effective class of a skill. Anything not scripted is derived
//! the way the hub derives it: a `skill.<name>` contract is available when the host announces
//! that skill, a compiled contract is available, and the effective class is the declared one.
//! It never evaluates policy and never approves.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::capability::{
    Availability, CatalogSnapshot, CatalogueGap, CatalogueState, ContractId, ContractIndex,
    EffectClass, builtin_contracts,
};
use domovoi_core::ports::{AgentLink, CapabilityRegistry};
use domovoi_core::{HostId, SkillName, SkillSummary};
use jiff::Timestamp;

use crate::link::FakeAgentLink;
use crate::lock;

/// A registry whose every answer is scripted by the test.
pub struct FakeCapabilityRegistry {
    contracts: Arc<ContractIndex>,
    /// The fake link whose announced skills answer when nothing is scripted here.
    link: Option<Arc<FakeAgentLink>>,
    skills: Mutex<BTreeMap<HostId, Vec<SkillSummary>>>,
    disabled: Mutex<BTreeSet<(HostId, SkillName)>>,
    availability: Mutex<BTreeMap<(HostId, ContractId), Availability>>,
    effects: Mutex<BTreeMap<(HostId, SkillName), EffectClass>>,
    catalogs: Mutex<BTreeMap<HostId, CatalogSnapshot>>,
}

impl FakeCapabilityRegistry {
    /// A registry over `contracts`, with no host announcing anything.
    #[must_use]
    pub fn new(contracts: Arc<ContractIndex>) -> Self {
        Self {
            contracts,
            link: None,
            skills: Mutex::new(BTreeMap::new()),
            disabled: Mutex::new(BTreeSet::new()),
            availability: Mutex::new(BTreeMap::new()),
            effects: Mutex::new(BTreeMap::new()),
            catalogs: Mutex::new(BTreeMap::new()),
        }
    }

    /// A registry that offers what `link` announces, minus what a test switches off.
    ///
    /// This is what a harness wants: the skills the fake agent has are the skills the hub
    /// would enable, so a test that only sets the link's skills keeps working.
    #[must_use]
    pub fn following(link: Arc<FakeAgentLink>) -> Self {
        let mut registry = Self::empty();
        registry.link = Some(link);
        registry
    }

    /// The skills `host` announces: the link's when one is attached, else the scripted set.
    fn announced(&self, host: HostId) -> Vec<SkillSummary> {
        match &self.link {
            Some(link) => link.skills(host),
            None => lock(&self.skills).get(&host).cloned().unwrap_or_default(),
        }
    }

    /// A registry over the hub's compiled contracts.
    #[must_use]
    pub fn empty() -> Self {
        // The compiled set is valid by construction; a failure here would leave an empty
        // index, which offers nothing rather than offering the wrong thing.
        match ContractIndex::new(builtin_contracts()) {
            Ok(index) => Self::new(Arc::new(index)),
            Err(_) => Self::new(Arc::new(ContractIndex::default())),
        }
    }

    /// The skills `host` announces.
    pub fn set_skills(&self, host: HostId, skills: Vec<SkillSummary>) {
        lock(&self.skills).insert(host, skills);
    }

    /// Switches one skill off on `host`, as the owner's setting would.
    pub fn disable(&self, host: HostId, skill: SkillName) {
        lock(&self.disabled).insert((host, skill));
    }

    /// Forces the answer for one `(host, contract)` pair.
    pub fn set_availability(&self, host: HostId, contract: &ContractId, state: Availability) {
        lock(&self.availability).insert((host, contract.clone()), state);
    }

    /// Forces the effective class of one skill on `host`, as a raised floor would.
    pub fn set_effect(&self, host: HostId, skill: SkillName, effect: EffectClass) {
        lock(&self.effects).insert((host, skill), effect);
    }

    /// The catalogue snapshot `host` reports.
    pub fn set_catalog(&self, host: HostId, snapshot: CatalogSnapshot) {
        lock(&self.catalogs).insert(host, snapshot);
    }

    /// Whether the test switched `skill` off on `host`.
    #[must_use]
    pub fn is_disabled(&self, host: HostId, skill: &SkillName) -> bool {
        lock(&self.disabled).contains(&(host, skill.clone()))
    }
}

impl Default for FakeCapabilityRegistry {
    fn default() -> Self {
        Self::empty()
    }
}

#[async_trait]
impl CapabilityRegistry for FakeCapabilityRegistry {
    async fn availability(&self, host: HostId, contract: &ContractId) -> Availability {
        if let Some(name) = contract.skill_name()
            && self.is_disabled(host, &name)
        {
            return Availability::DisabledByHub;
        }
        if let Some(scripted) = lock(&self.availability).get(&(host, contract.clone())) {
            return scripted.clone();
        }
        if let Some(name) = contract.skill_name() {
            let announced = self
                .announced(host)
                .iter()
                .any(|summary| summary.name == name);
            if !announced {
                return Availability::NotInstalled;
            }
            return Availability::Available;
        }
        if self.contracts.get(contract).is_none() {
            return Availability::NotInstalled;
        }
        Availability::Available
    }

    async fn catalog(&self, host: HostId) -> CatalogSnapshot {
        match lock(&self.catalogs).get(&host) {
            Some(snapshot) => snapshot.clone(),
            None => CatalogSnapshot {
                host,
                announced_hash: None,
                state: CatalogueState::Partial {
                    gap: CatalogueGap::NotWalked,
                },
                entries: BTreeMap::new(),
                unknown: Vec::new(),
                pages: 0,
                session_since: None,
                at: Timestamp::constant(1_700_000_000, 0),
            },
        }
    }

    fn contracts(&self) -> &ContractIndex {
        &self.contracts
    }

    fn effective_effect(&self, host: HostId, skill: &SkillName) -> Option<EffectClass> {
        if let Some(effect) = lock(&self.effects).get(&(host, skill.clone())) {
            return Some(*effect);
        }
        self.announced(host)
            .into_iter()
            .find(|summary| &summary.name == skill)
            .map(|summary| EffectClass::from(summary.risk))
    }

    fn enabled(&self, host: HostId, contract: &ContractId) -> bool {
        contract
            .skill_name()
            .is_none_or(|name| !self.is_disabled(host, &name))
    }

    fn enabled_skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.announced(host)
            .into_iter()
            .filter(|summary| !self.is_disabled(host, &summary.name))
            .collect()
    }
}
