//! A fake `ShadowEnvironment` for the recorder tests.
//!
//! It lives with the engine tests because the testkit cannot depend on the engine. Defaults:
//! the compiled contracts plus the skills a test adds, the testkit's host identity and the
//! host id as its name, ceiling `Absent`, `runs_as_root` `Some(false)`, no facts, containment
//! not applicable, autonomy not modelled. Every answer is overridable per host.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::capability::{
    CapabilityContract, CeilingSummary, ContractId, ContractIndex, ContractMeta, FactId,
    TargetName, TargetQuery, builtin_contracts, skill_contract,
};
use domovoi_core::ports::TargetResolver;
use domovoi_core::{HostId, SkillSummary};
use domovoi_engine::policy::FactState;
use domovoi_engine::policy_shadow::{
    AutonomyReading, CeilingReading, ContainmentReading, GapReason, HostTarget, ShadowEnvironment,
};
use domovoi_testkit::FakeTargetResolver;

#[derive(Default)]
struct State {
    skills: Vec<CapabilityContract>,
    names: BTreeMap<HostId, TargetName>,
    ceilings: BTreeMap<HostId, CeilingReading>,
    root: BTreeMap<HostId, Option<bool>>,
    facts: BTreeMap<HostId, BTreeMap<FactId, FactState>>,
}

pub struct FakeShadowEnvironment {
    resolver: FakeTargetResolver,
    state: Mutex<State>,
}

impl FakeShadowEnvironment {
    pub fn new() -> Self {
        Self {
            resolver: FakeTargetResolver::new(),
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Makes the skill's contract known.
    pub fn add_skill(&self, summary: &SkillSummary) {
        self.state().skills.push(skill_contract(summary, None));
    }

    pub fn set_host_name(&self, host: HostId, name: &str) {
        self.state()
            .names
            .insert(host, TargetName::parse(name).unwrap());
    }

    pub fn fail_host(&self, host: HostId) {
        self.resolver.fail(host);
    }

    pub fn set_ceiling(&self, host: HostId, reading: CeilingReading) {
        self.state().ceilings.insert(host, reading);
    }

    pub fn set_runs_as_root(&self, host: HostId, root: Option<bool>) {
        self.state().root.insert(host, root);
    }

    pub fn set_fact(&self, host: HostId, fact: &str, state: FactState) {
        self.state()
            .facts
            .entry(host)
            .or_default()
            .insert(FactId::parse(fact).unwrap(), state);
    }

    fn contracts(&self) -> Vec<CapabilityContract> {
        let mut all = builtin_contracts();
        all.extend(self.state().skills.clone());
        all
    }

    fn index(&self) -> ContractIndex {
        ContractIndex::new(self.contracts()).unwrap()
    }
}

#[async_trait]
impl ShadowEnvironment for FakeShadowEnvironment {
    async fn contract(&self, _host: HostId, id: &ContractId) -> Option<ContractMeta> {
        self.index().meta(id)
    }

    async fn host_target(&self, host: HostId) -> Result<HostTarget, GapReason> {
        let resolved = self
            .resolver
            .resolve(TargetQuery::Host { host })
            .await
            .map_err(|_| GapReason::Unresolvable)?;
        let name = self
            .state()
            .names
            .get(&host)
            .cloned()
            .unwrap_or(resolved.name);
        Ok(HostTarget {
            name,
            identity: resolved.identity,
        })
    }

    async fn ceiling(&self, host: HostId) -> CeilingReading {
        self.state()
            .ceilings
            .get(&host)
            .cloned()
            .unwrap_or(CeilingReading::Absent)
    }

    async fn runs_as_root(&self, host: HostId) -> Option<bool> {
        self.state().root.get(&host).copied().unwrap_or(Some(false))
    }

    async fn facts(&self, host: HostId, wanted: &BTreeSet<FactId>) -> BTreeMap<FactId, FactState> {
        self.state()
            .facts
            .get(&host)
            .map(|known| {
                known
                    .iter()
                    .filter(|(fact, _)| wanted.contains(fact))
                    .map(|(fact, state)| (fact.clone(), *state))
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn containment(&self, _host: HostId) -> ContainmentReading {
        ContainmentReading::NotApplicable
    }

    async fn autonomy(&self) -> AutonomyReading {
        AutonomyReading::NotModelled
    }

    async fn publication_index(&self) -> ContractIndex {
        self.index()
    }

    async fn ceilings(&self) -> BTreeMap<HostId, CeilingSummary> {
        self.state()
            .ceilings
            .iter()
            .filter_map(|(host, reading)| match reading {
                CeilingReading::Reported(summary) => Some((*host, summary.clone())),
                _ => None,
            })
            .collect()
    }
}

/// A skill summary named `name` at risk `risk`, with no parameters.
pub fn skill(name: &str, risk: domovoi_core::Risk) -> SkillSummary {
    domovoi_testkit::build::manifest(name, risk)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}
