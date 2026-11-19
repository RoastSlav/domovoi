//! What the policy tests need besides the API: an environment that knows the skills a test
//! announces, and a way to record shadow decisions for steps without running an operation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::capability::{
    CapabilityContract, CeilingSummary, ContractId, ContractIndex, ContractMeta, FactId,
    TargetQuery, builtin_contracts, skill_contract,
};
use domovoi_core::policy_store::{
    PolicyWrite, ShadowCandidate, ShadowRecord, ShadowStatus, SweepStats,
};
use domovoi_core::ports::{StateWrite, TargetResolver, Transaction, UnitOfWork};
use domovoi_core::{CheckKey, ExecutionId, HostId, OperationId, PinnedSubject, Risk, SkillSummary};
use domovoi_engine::policy::{FactState, Rule, evaluate, evaluate_shadow};
use domovoi_engine::policy_shadow::{
    AutonomyReading, CandidateDoc, CeilingReading, ContainmentReading, DecisionEnvelope, GapReason,
    HostTarget, InputsEnvelope, ShadowEnvironment, Verdict, build, rules_hash,
};
use domovoi_testkit::FakeTargetResolver;

use crate::api_harness::Api;

/// The skill every policy test announces, and its contract id.
pub const SKILL: &str = "restart_plex";
/// The contract id of [`SKILL`].
pub const CONTRACT: &str = "skill.restart_plex";

/// Compiled contracts plus the skills a test adds; every host resolves, no ceiling is
/// reported and no fact is known.
pub struct PolicyFakeEnv {
    resolver: FakeTargetResolver,
    skills: Mutex<Vec<CapabilityContract>>,
}

impl PolicyFakeEnv {
    pub fn new() -> Self {
        let env = Self {
            resolver: FakeTargetResolver::new(),
            skills: Mutex::new(Vec::new()),
        };
        env.add_skill(&skill(SKILL, Risk::Write));
        env
    }

    pub fn add_skill(&self, summary: &SkillSummary) {
        self.skills
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(skill_contract(summary, None));
    }

    fn index(&self) -> ContractIndex {
        let mut all = builtin_contracts();
        all.extend(
            self.skills
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        );
        ContractIndex::new(all).unwrap()
    }
}

/// A skill summary named `name` at `risk`.
pub fn skill(name: &str, risk: Risk) -> SkillSummary {
    domovoi_testkit::build::manifest(name, risk)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
}

#[async_trait]
impl ShadowEnvironment for PolicyFakeEnv {
    async fn contract(&self, _host: HostId, id: &ContractId) -> Option<ContractMeta> {
        self.index().meta(id)
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

    async fn ceiling(&self, _host: HostId) -> CeilingReading {
        CeilingReading::Absent
    }

    async fn runs_as_root(&self, _host: HostId) -> Option<bool> {
        Some(false)
    }

    async fn facts(&self, _: HostId, _: &BTreeSet<FactId>) -> BTreeMap<FactId, FactState> {
        BTreeMap::new()
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
        BTreeMap::new()
    }
}

/// Records one shadow decision for a step of [`CONTRACT`] on the harness host, as the recorder
/// would have under `rules` published as `version` (0 for none). Returns the step's id.
pub async fn record_step(a: &Api, rules: &[Rule], version: u32) -> ExecutionId {
    let host = a.nas.id;
    let now = a.now();
    let candidate = ShadowCandidate {
        execution: ExecutionId::new(),
        operation: OperationId::new(),
        host,
        contract: ContractId::parse(CONTRACT).unwrap(),
        contract_version: 1,
        check: CheckKey::parse("container.down").unwrap(),
        target: None,
        args: BTreeMap::new(),
        pinned: vec![PinnedSubject {
            host,
            check: CheckKey::parse("container.down").unwrap(),
            target: None,
        }],
        created_at: now,
    };
    let built = build(&candidate, rules, &*a.policy_env, now).await;
    assert_eq!(built.verdict, Verdict::Evaluate);
    let (request, inputs) = (built.request.unwrap(), built.inputs.unwrap());
    let enforced = DecisionEnvelope::from_result(&evaluate(rules, &request));
    let shadow = DecisionEnvelope::from_result(&evaluate_shadow(rules, &request));
    let record = ShadowRecord {
        execution: candidate.execution,
        operation: candidate.operation,
        host,
        contract: CONTRACT.to_owned(),
        step_created_at: now,
        evaluated_at: now,
        status: ShadowStatus::Evaluated,
        reason: None,
        policy_version: version,
        rules_hash: rules_hash(&domovoi_engine::policy_shadow::stored_rules(rules)),
        enforced_outcome: Some(enforced.outcome_text().to_owned()),
        shadow_outcome: Some(shadow.outcome_text().to_owned()),
        degraded: !built.gaps.is_empty(),
        candidate: serde_json::to_string(&CandidateDoc::from_candidate(&candidate)).unwrap(),
        inputs: Some(serde_json::to_string(&InputsEnvelope::new(inputs)).unwrap()),
        enforced: Some(enforced.to_json()),
        shadow: Some(shadow.to_json()),
        gaps: serde_json::to_string(&built.gaps).unwrap(),
    };
    let write = PolicyWrite::RecordShadow {
        records: vec![record],
        sweep: SweepStats {
            evaluated: 1,
            unevaluable: 0,
            skipped: 0,
        },
        at: now,
        error: None,
    };
    a.store
        .commit(Transaction::new().write(StateWrite::Policy(write)))
        .await
        .unwrap();
    candidate.execution
}
