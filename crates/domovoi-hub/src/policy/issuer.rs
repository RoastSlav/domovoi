//! [`HubPermitIssuer`]: the guards, the evaluation and the one place a permit is built.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::capability::containment::BLOCK_SOURCE_ID;
use domovoi_core::capability::{TargetName, is_policy_dispatchable};
use domovoi_core::policy_store::ShadowCandidate;
use domovoi_core::ports::AuditDraft;
use domovoi_core::ports::{
    Clock, NotIssued, PermitGrant, PermitIssuer, PermitOutcome, PermitRefusal, PermitRepo,
    PermitRequest, PolicyRepo, TargetResolver, Transaction, UnitOfWork,
};
use domovoi_core::{
    Actor, Authorization, CapabilityPlan, ExecutionRecord, PlanHash, PolicyPermit,
    PolicyPermitParts,
};
use domovoi_engine::containment::ContainmentDriver;
use domovoi_engine::containment::permit::{BlockFacts, BlockFactsError, block_facts};
use domovoi_engine::policy::{Outcome, evaluate};
use domovoi_engine::policy_shadow::{
    AutonomyReading, DecisionEnvelope, ShadowEnvironment, Verdict, build, decode_rules,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use uuid::Uuid;

use super::guards;

/// The knobs of `[policy]` the issuer reads.
#[derive(Debug, Clone, Copy)]
pub struct PermitSettings {
    /// How long a permit stays valid, capped by the step's own window.
    pub ttl: SignedDuration,
    /// Permits per rolling hour for the whole hub; `0` issues none.
    pub max_per_hour: u32,
}

/// What the issuer reads and writes.
pub struct IssuerDeps {
    /// The published policy.
    pub policy: Arc<dyn PolicyRepo>,
    /// The pause switch, the permit counts and the unknown-outcome block.
    pub permits: Arc<dyn PermitRepo>,
    /// Re-resolves the unit through the helper.
    pub resolver: Arc<dyn TargetResolver>,
    /// Builds the evaluator's inputs: the live reading of the host, the ceiling and autonomy.
    pub env: Arc<dyn ShadowEnvironment>,
    /// Where a withheld permit is audited.
    pub uow: Arc<dyn UnitOfWork>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// The `[policy]` settings.
    pub settings: PermitSettings,
    /// The containment service, for the verified helper identity and the drills a block needs;
    /// `None` leaves every block to a person.
    pub contain: Option<Arc<ContainmentDriver>>,
}

/// What the plan binds, read now, and the evaluator inputs that come with it.
struct Ground {
    /// The plan hash recomputed now.
    plan: PlanHash,
    /// The arguments the evaluator reads.
    args: std::collections::BTreeMap<String, String>,
    /// The block's verified facts; `None` for a unit restart.
    block: Option<BlockFacts>,
}

/// Issues policy permits.
pub struct HubPermitIssuer {
    deps: IssuerDeps,
}

impl HubPermitIssuer {
    /// An issuer over `deps`.
    #[must_use]
    pub fn new(deps: IssuerDeps) -> Self {
        Self { deps }
    }
}

fn withheld(refusal: PermitRefusal) -> NotIssued {
    NotIssued::Withheld(refusal)
}

#[async_trait]
impl PermitIssuer for HubPermitIssuer {
    async fn issue(&self, request: PermitRequest) -> PermitOutcome {
        match self.consider(&request).await {
            Ok(grant) => PermitOutcome::Issued(grant),
            Err(not_issued) => {
                if let NotIssued::Withheld(refusal) = &not_issued
                    && refusal.is_audited()
                {
                    self.audit_withheld(&request, refusal).await;
                }
                PermitOutcome::NotIssued(not_issued)
            }
        }
    }
}

impl HubPermitIssuer {
    /// The guards, in order; a permit is built only after every one passed and the evaluator
    /// allowed.
    async fn consider(&self, request: &PermitRequest) -> Result<PermitGrant, NotIssued> {
        let d = &self.deps;
        let row = &request.execution;
        let now = d.clock.now();
        let unreadable = |_| withheld(PermitRefusal::InputsUnavailable);

        // 1. Only a contract on the closed list may be permitted.
        if !is_policy_dispatchable(&row.contract) {
            return Err(withheld(PermitRefusal::NotPolicyDispatchable));
        }
        // 2. Paused autonomy issues nothing.
        if d.permits.autonomy().await.map_err(unreadable)?.paused {
            return Err(withheld(PermitRefusal::Paused));
        }
        // 3. An earlier restart of this unit whose outcome nobody resolved blocks unattended reuse.
        if let Some(target) = &row.target
            && let Some(unknown) = d
                .permits
                .unresolved_unknown(row.host, &row.contract, target)
                .await
                .map_err(unreadable)?
        {
            return Err(withheld(PermitRefusal::UnresolvedUnknown(unknown.step)));
        }
        // 4. Re-resolve what the plan binds now: a configuration change since the proposal
        // withholds the permit and leaves the approval path. A block also needs a verified
        // helper identity and passed drills, or it is left to a person.
        let ground = self.ground(row, now).await?;
        if ground.plan != row.plan_hash {
            return Err(withheld(PermitRefusal::PlanChanged));
        }
        // 5. The published policy. Shadow rules are never enforced: `evaluate` selects only the
        // enforced ones.
        let version = d
            .policy
            .active()
            .await
            .map_err(unreadable)?
            .ok_or_else(|| withheld(PermitRefusal::NoPublishedPolicy))?;
        let stored = d.policy.rules(version.id).await.map_err(unreadable)?;
        let rules =
            decode_rules(&stored).map_err(|_| withheld(PermitRefusal::InputsUnavailable))?;
        // 6. The live inputs and the decision.
        let candidate = ShadowCandidate {
            execution: row.execution,
            operation: row.operation,
            host: row.host,
            contract: row.contract.clone(),
            contract_version: row.contract_version,
            check: row.check.clone(),
            target: row.target.clone(),
            args: ground.args.clone(),
            pinned: request.pinned.clone(),
            created_at: row.created_at,
        };
        let built = build(&candidate, &rules, &*d.env, now).await;
        let (Verdict::Evaluate, Some(mut input)) = (built.verdict, built.request) else {
            return Err(withheld(PermitRefusal::InputsUnavailable));
        };
        // The evaluator's containment input and the requested TTL come from the same verified
        // reading the permit is bound to; without a TTL a ceiling's `max_ttl` would cover anything.
        if let Some(block) = &ground.block {
            input.containment = block.containment.clone();
            input.requested_ttl = Some(block.ttl);
        }
        let result = evaluate(&rules, &input);
        let document = DecisionEnvelope::from_result(&result)
            .to_json()
            .map_err(|_| withheld(PermitRefusal::InputsUnavailable))?;
        let Ok(decision) = result else {
            return Err(withheld(PermitRefusal::InputsUnavailable));
        };
        if decision.outcome != Outcome::Allow {
            return Err(NotIssued::Evaluated {
                outcome: decision.outcome.name().to_owned(),
                decision: document,
            });
        }
        // A rule whose breaker is open is suspended: evaluate again as broken, so the trace shows
        // the hard limit, and withhold. An `Allow` that survives that is itself withheld.
        let AutonomyReading::Reported(facts) = d.env.autonomy().await;
        if let Some(rule) = decision
            .winners
            .iter()
            .find(|winner| facts.suspended_rules.contains(winner.as_str()))
        {
            let mut broken = input.clone();
            broken.autonomy.circuit_broken = true;
            if let Ok(again) = evaluate(&rules, &broken) {
                tracing::debug!(rule = %rule, outcome = %again.outcome.name(), "a suspended rule was re-evaluated");
            }
            return Err(withheld(PermitRefusal::RuleSuspended(rule.to_string())));
        }
        // 7. Rates.
        let winners: Vec<String> = decision.winners.iter().map(ToString::to_string).collect();
        guards::check(
            &*d.permits,
            now,
            row.host,
            &row.contract,
            &winners,
            decision.constraints.max_per_hour,
            d.settings.max_per_hour,
        )
        .await
        .map_err(withheld)?;
        // 8. The one place a permit is built: from an `Allow`, after every guard.
        let number = version
            .number
            .ok_or_else(|| withheld(PermitRefusal::NoPublishedPolicy))?;
        let expires_at = now
            .checked_add(d.settings.ttl)
            .map_or(row.expires_at, |t| t.min(row.expires_at));
        let permit = PolicyPermit::issue(PolicyPermitParts {
            decision_id: &Uuid::now_v7().to_string(),
            rule_ids: winners,
            policy_version: &number.to_string(),
            plan_hash: row.plan_hash,
            issued_at: now,
            expires_at,
            ceiling_hash: decision.ceiling_hash.as_deref().unwrap_or_default(),
            safety_identity: ground
                .block
                .as_ref()
                .map(|block| block.safety_identity.as_str()),
        })
        .map_err(|_| withheld(PermitRefusal::InputsUnavailable))?;
        Ok(PermitGrant {
            authorization: Authorization::policy(permit),
            decision: document,
        })
    }

    /// What the plan binds, read now: the unit's canonical identity for a restart, or the verified
    /// helper identity, the passed drills and the evaluator's containment input for a block.
    async fn ground(&self, row: &ExecutionRecord, now: Timestamp) -> Result<Ground, NotIssued> {
        let d = &self.deps;
        if row.contract.as_str() == BLOCK_SOURCE_ID {
            return self.block_ground(row, now).await;
        }
        let typed = row
            .args
            .get("unit")
            .and_then(|text| TargetName::parse(text).ok())
            .ok_or_else(|| withheld(PermitRefusal::InputsUnavailable))?;
        let unit =
            d.resolver
                .resolve_unit(row.host, &typed)
                .await
                .map_err(|error| match error {
                    domovoi_core::capability::TargetError::UnitRefused { refusal, .. } => {
                        withheld(PermitRefusal::IdentityRefused(refusal))
                    }
                    _ => withheld(PermitRefusal::InputsUnavailable),
                })?;
        let plan = CapabilityPlan::unit_restart(
            row.host,
            &unit.target.host_key,
            &unit.target.identity,
            &unit.identity_hash,
            &row.args,
            row.execution,
        )
        .hash();
        Ok(Ground {
            plan,
            args: row.args.clone(),
            block: None,
        })
    }

    /// The block's ground: refused unless containment is set up, the drills passed and the helper
    /// is verified. A person may still approve it.
    async fn block_ground(
        &self,
        row: &ExecutionRecord,
        now: Timestamp,
    ) -> Result<Ground, NotIssued> {
        let Some(driver) = &self.deps.contain else {
            return Err(withheld(PermitRefusal::InputsUnavailable));
        };
        if !driver.drills_passed(row.host) {
            return Err(withheld(PermitRefusal::DrillsNotPassed));
        }
        let facts = block_facts(driver, row, now)
            .await
            .map_err(|error| match error {
                BlockFactsError::Unverified(reason) => {
                    withheld(PermitRefusal::NotVerified(reason.token()))
                }
                BlockFactsError::BadRequest(_) | BlockFactsError::Unreadable(_) => {
                    withheld(PermitRefusal::InputsUnavailable)
                }
            })?;
        Ok(Ground {
            plan: facts.plan,
            args: facts.args.clone(),
            block: Some(facts),
        })
    }

    /// Audits a withheld permit whose reason is a safety guard. A failed audit write is logged:
    /// nothing was issued either way.
    async fn audit_withheld(&self, request: &PermitRequest, refusal: &PermitRefusal) {
        let now: Timestamp = self.deps.clock.now();
        let draft = AuditDraft::new(
            Actor::System("policy".to_owned()),
            now,
            "policy.permit_withheld".to_owned(),
            format!("execution:{}", request.execution.execution),
            json!({ "token": refusal.token() }),
        );
        if let Err(error) = self.deps.uow.commit(Transaction::new().audit(draft)).await {
            tracing::warn!(%error, "could not audit a withheld permit");
        }
    }
}
