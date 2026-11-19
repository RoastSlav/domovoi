//! Wires the policy use cases and, when `[policy] shadow` is on, the shadow recorder.
//!
//! [`HubShadowEnvironment`] answers the recorder's questions from what the hub already knows:
//! the capability registry, the target resolver and the live sessions' announcements. It only
//! reads. Where a host input does not exist yet (the unattended ceiling before the agents
//! report one, fact producers, the helper), it says so and the recorder substitutes the
//! fail-closed value and records the gap.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_config::HubConfig;
use domovoi_core::capability::{
    CapabilityContract, CeilingSummary, ContractId, ContractIndex, ContractMeta, FactId,
    SkillFloor, TargetQuery, skill_contract,
};
use domovoi_core::ports::{CapabilityRegistry, Clock, HostRepo, TargetResolver};
use domovoi_core::{HostId, SkillSummary};
use domovoi_engine::Evidence;
use domovoi_engine::policy::FactState;
use domovoi_engine::policy_shadow::{
    AutonomyReading, CeilingReading, ContainmentReading, GapReason, HostTarget, ShadowEnvironment,
    ShadowRecorder,
};
use domovoi_engine::targets::TargetService;
use domovoi_store::Store;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::auth::AuthState;
use crate::capability::{CeilingParse, HubRegistry, parse_ceiling};
use crate::services::{PolicyDeps, PolicyService};

/// The policy service and, when shadow recording is on, the task that runs the recorder.
pub(crate) struct PolicyParts {
    /// The use cases.
    pub(crate) service: PolicyService,
    /// The recorder task; `None` while `[policy] shadow` is off.
    pub(crate) task: Option<JoinHandle<()>>,
}

/// Builds the policy service, and starts the recorder only when `config.policy.shadow` is set.
pub(crate) fn wire(
    store: &Arc<Store>,
    registry: &Arc<HubRegistry>,
    evidence: &Evidence,
    config: &HubConfig,
    clock: &Arc<dyn Clock>,
    auth: &Arc<AuthState>,
    shutdown: &CancellationToken,
) -> PolicyParts {
    let resolver = Arc::new(TargetService::new(
        Arc::new(evidence.clone()),
        store.clone(),
    ));
    let env: Arc<dyn ShadowEnvironment> = Arc::new(HubShadowEnvironment::new(
        registry.clone(),
        resolver,
        store.clone(),
    ));
    let shadow = config.policy.shadow;
    let task = shadow.then(|| {
        let recorder =
            ShadowRecorder::new(store.clone(), store.clone(), env.clone(), clock.clone());
        tokio::spawn(recorder.run(shutdown.clone()))
    });
    let service = PolicyService::new(PolicyDeps {
        repo: store.clone(),
        uow: store.clone(),
        env,
        auth: auth.clone(),
        clock: clock.clone(),
        shadow_enabled: shadow,
    });
    PolicyParts { service, task }
}

/// What the shadow recorder and the publication review ask the hub.
pub struct HubShadowEnvironment {
    registry: Arc<HubRegistry>,
    resolver: Arc<dyn TargetResolver>,
    hosts: Arc<dyn HostRepo>,
}

impl HubShadowEnvironment {
    /// The environment over the capability registry, a target resolver and the host list.
    #[must_use]
    pub fn new(
        registry: Arc<HubRegistry>,
        resolver: Arc<dyn TargetResolver>,
        hosts: Arc<dyn HostRepo>,
    ) -> Self {
        Self {
            registry,
            resolver,
            hosts,
        }
    }

    /// The contract of one announced skill on `host`, its class raised by any hub floor.
    fn skill_on(&self, host: HostId, summary: &SkillSummary) -> CapabilityContract {
        let floor = self
            .registry
            .effective_effect(host, &summary.name)
            .and_then(|class| SkillFloor::new(class).ok());
        skill_contract(summary, floor)
    }

    /// The parsed ceiling `host` reported, if it reported a valid one.
    fn reported_ceiling(&self, host: HostId) -> CeilingReading {
        let Some(view) = self.registry.view(host) else {
            return CeilingReading::HostNotConnected;
        };
        match parse_ceiling(view.words.ceiling_summary.as_deref()) {
            CeilingParse::Parsed(summary) => CeilingReading::Reported(summary),
            CeilingParse::Absent | CeilingParse::Malformed { .. } => CeilingReading::Absent,
        }
    }

    async fn host_ids(&self) -> Vec<HostId> {
        match self.hosts.list_hosts().await {
            Ok(hosts) => hosts.into_iter().map(|host| host.id).collect(),
            Err(error) => {
                tracing::warn!(%error, "could not list hosts for the policy review");
                Vec::new()
            }
        }
    }
}

/// The policy-facing summary of a contract.
fn meta_of(contract: &CapabilityContract) -> ContractMeta {
    ContractMeta {
        id: contract.id.clone(),
        category: contract.category.clone(),
        effect: contract.effect,
        target_kind: contract.target_kind,
        safety_facts: contract.safety_facts.clone(),
        constrainable_args: contract.constrainable_args.clone(),
    }
}

#[async_trait]
impl ShadowEnvironment for HubShadowEnvironment {
    async fn contract(&self, host: HostId, id: &ContractId) -> Option<ContractMeta> {
        let Some(name) = id.skill_name() else {
            return self.registry.contracts().meta(id);
        };
        let view = self.registry.view(host)?;
        let summary = view.skills.iter().find(|skill| skill.name == name)?;
        Some(meta_of(&self.skill_on(host, summary)))
    }

    async fn host_target(&self, host: HostId) -> Result<HostTarget, GapReason> {
        let resolved = self
            .resolver
            .resolve(TargetQuery::Host { host })
            .await
            .map_err(|_| GapReason::Unresolvable)?;
        Ok(HostTarget {
            name: resolved.name,
            identity: resolved.identity,
        })
    }

    async fn ceiling(&self, host: HostId) -> CeilingReading {
        self.reported_ceiling(host)
    }

    async fn runs_as_root(&self, host: HostId) -> Option<bool> {
        self.registry.view(host).map(|view| view.words.runs_as_root)
    }

    async fn facts(
        &self,
        _host: HostId,
        _wanted: &BTreeSet<FactId>,
    ) -> BTreeMap<FactId, FactState> {
        BTreeMap::new()
    }

    async fn containment(&self, _host: HostId) -> ContainmentReading {
        ContainmentReading::Unavailable
    }

    async fn autonomy(&self) -> AutonomyReading {
        AutonomyReading::NotModelled
    }

    async fn publication_index(&self) -> ContractIndex {
        let compiled: Vec<CapabilityContract> = self.registry.contracts().iter().cloned().collect();
        // One representative per announced skill name; the strictest effective class wins.
        let mut skills: BTreeMap<ContractId, CapabilityContract> = BTreeMap::new();
        for host in self.host_ids().await {
            let Some(view) = self.registry.view(host) else {
                continue;
            };
            for summary in &view.skills {
                let contract = self.skill_on(host, summary);
                let stricter = skills
                    .get(&contract.id)
                    .is_none_or(|known| contract.effect > known.effect);
                if stricter {
                    skills.insert(contract.id.clone(), contract);
                }
            }
        }
        let mut all = compiled.clone();
        all.extend(skills.into_values());
        ContractIndex::new(all).unwrap_or_else(|error| {
            tracing::warn!(%error, "announced skills do not form a valid index; reviewing compiled contracts only");
            ContractIndex::new(compiled).unwrap_or_default()
        })
    }

    async fn ceilings(&self) -> BTreeMap<HostId, CeilingSummary> {
        let mut found = BTreeMap::new();
        for host in self.host_ids().await {
            if let CeilingReading::Reported(summary) = self.reported_ceiling(host) {
                found.insert(host, summary);
            }
        }
        found
    }
}
