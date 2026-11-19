//! The operation service behind the API harness: a real service over the harness's store,
//! clock and fake agent link, with a host that offers `restart_container` (a write skill)
//! and `prune_images` (a destructive one). Its runner claims nothing, so a test reads what a
//! commit queued instead of racing the dispatch.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    AggregateKind, ClaimBatch, EffectKind, EffectQueue, EffectRead, EffectRecord, EffectStatus,
    Presence, StoreError,
};
use domovoi_core::{
    Actor, AgentIdentity, CheckKey, ContractId, ExecutionIntent, Host, JournalEpoch, OperationId,
    OperationKind, ParamKind, ParamName, ParamSpec, PinnedSubject, Risk, SkillSummary, StepId,
    TargetKey,
};
use domovoi_engine::TriageHandle;
use domovoi_engine::operations::{
    ExecutionRequest, InvestigateDeps, OperationDeps, OperationHandle, OperationSeed,
    OperationService, OperationSettings,
};
use domovoi_engine::{Evidence, PatternRedactor};
use domovoi_store::Store;
use domovoi_testkit::build::manifest;
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use domovoi_testkit::{FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakeTargetResolver};
use jiff::{SignedDuration, Timestamp};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::api_harness::Api;

/// An effect queue that hands the runner nothing.
struct Closed;

#[async_trait]
impl EffectQueue for Closed {
    async fn claim_due(
        &self,
        _runner: &str,
        _kinds: &[EffectKind],
        _aggregates: &[AggregateKind],
        _now: Timestamp,
        _lease: SignedDuration,
        _limit: u16,
    ) -> Result<ClaimBatch, StoreError> {
        Ok(ClaimBatch::default())
    }

    async fn revert_claimed_at_start(
        &self,
        _aggregates: &[AggregateKind],
    ) -> Result<u64, StoreError> {
        Ok(0)
    }
}

/// The write skill: restarts a named container.
pub fn restart_skill() -> SkillSummary {
    let mut manifest = manifest("restart_container", Risk::Write).unwrap();
    manifest.params = vec![ParamSpec {
        name: ParamName::parse("container").unwrap(),
        description: "The container to restart.".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^[a-z0-9_-]+$".to_owned(),
            max_len: 64,
        },
    }];
    manifest.summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

/// The destructive skill: removes unused images.
pub fn prune_skill() -> SkillSummary {
    manifest("prune_images", Risk::Destructive)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

/// Starts the service over `store` for `host`, which gets an enrolled agent and a catalogue.
pub async fn spawn(
    store: &Arc<Store>,
    link: &Arc<FakeAgentLink>,
    clock: &Arc<FakeClock>,
    shutdown: &CancellationToken,
    host: &Host,
    triage: Option<TriageHandle>,
) -> OperationHandle {
    let started = domovoi_core::ports::Clock::now(&**clock);
    link.set_presence(
        host.id,
        Presence::Connected {
            since: started,
            version: "0.1.0".to_owned(),
        },
    );
    link.set_skills(host.id, vec![restart_skill(), prune_skill()]);
    link.set_journal_epoch(host.id, JournalEpoch::from_uuid(Uuid::now_v7()));
    domovoi_core::ports::HostRepo::put_agent(
        &**store,
        &AgentIdentity {
            agent: link.agent_id(host.id),
            host: host.id,
            public_key: [7; 32],
            version: "0.1.0".to_owned(),
            enrolled_at: started,
            revoked_at: None,
        },
    )
    .await
    .unwrap();
    let registry = Arc::new(FakeCapabilityRegistry::following(link.clone()));
    let caps = action_caps(link.clone());
    OperationService::spawn(
        OperationDeps {
            operations: store.clone(),
            uow: store.clone(),
            effects: Arc::new(Closed),
            effect_read: store.clone(),
            advisories: store.clone(),
            incidents: store.clone(),
            hosts: store.clone(),
            evidence: Evidence::new(evidence_caps(link.clone()), clock.clone(), registry.clone()),
            registry,
            resolver: Arc::new(FakeTargetResolver::new()),
            observer: observer_of(link.clone()),
            journal: caps.journal,
            handoff: caps.handoff,
            rerun: caps.rerun,
            redactor: Arc::new(PatternRedactor::default()),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
            investigate: triage.map(|triage| InvestigateDeps { triage }),
        },
        OperationSettings::default(),
    )
    .0
}

/// A request to run `skill` with `args` on `host` about the `plex` container check.
pub fn request(host: &Host, skill: &str, args: &[(&str, &str)]) -> ExecutionRequest {
    ExecutionRequest {
        intent: ExecutionIntent {
            contract: ContractId::parse(&format!("skill.{skill}")).unwrap(),
            contract_version: 1,
            host: host.id,
            check: CheckKey::parse("container.down").unwrap(),
            target: Some(TargetKey::parse("plex").unwrap()),
            args: args
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<BTreeMap<_, _>>(),
            retry_of: None,
        },
        parent: None,
        caused_by: None,
    }
}

impl Api {
    /// The operation service. Panics when the API was built without one.
    pub fn ops(&self) -> &OperationHandle {
        self.operations.as_ref().expect("built with operations")
    }

    /// Creates an Investigate operation about `goal` on the `nas` host, linked to the
    /// harness's incident when `incident` is set.
    pub async fn create_operation(&self, goal: &str, incident: bool) -> OperationId {
        let seed = OperationSeed {
            kind: OperationKind::Investigate,
            goal: goal.to_owned(),
            pinned: vec![PinnedSubject {
                host: self.nas.id,
                check: CheckKey::parse("container.down").unwrap(),
                target: Some(TargetKey::parse("plex").unwrap()),
            }],
            incidents: if incident {
                vec![self.incident.id()]
            } else {
                Vec::new()
            },
            parent: None,
            budgets: None,
            first_steps: Vec::new(),
            effects: Vec::new(),
            actor: Actor::System("tests".to_owned()),
        };
        self.ops().create(seed).await.unwrap()
    }

    /// Proposes `skill` on the `nas` host in `op` and returns the Execution step.
    pub async fn propose_run(&self, op: OperationId, skill: &str, args: &[(&str, &str)]) -> StepId {
        self.ops()
            .propose_execution(op, request(&self.nas, skill, args))
            .await
            .unwrap()
    }

    /// Proposes the restart of `plex`.
    pub async fn propose_restart(&self, op: OperationId) -> StepId {
        self.propose_run(op, "restart_container", &[("container", "plex")])
            .await
    }

    /// The journal head, as the API reads it.
    pub async fn head_seq(&self) -> u64 {
        self.state.deps().journal.head_seq().await.unwrap()
    }

    /// Every effect of `status` in the store.
    pub async fn effects(&self, status: EffectStatus) -> Vec<EffectRecord> {
        EffectRead::effects_with_status(&*self.store, status, 500)
            .await
            .unwrap()
    }
}
