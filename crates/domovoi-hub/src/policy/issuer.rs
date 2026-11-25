//! [`HubPermitIssuer`]: the guards, the evaluation and the one place a permit is built.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::capability::{TargetName, is_policy_dispatchable};
use domovoi_core::policy_store::ShadowCandidate;
use domovoi_core::ports::AuditDraft;
use domovoi_core::ports::{
    Clock, NotIssued, PermitGrant, PermitIssuer, PermitOutcome, PermitRefusal, PermitRepo,
    PermitRequest, PolicyRepo, TargetResolver, Transaction, UnitOfWork,
};
use domovoi_core::{Actor, Authorization, CapabilityPlan, PolicyPermit, PolicyPermitParts};
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
        // 4. Re-resolve the unit now: a configuration change since the proposal withholds the
        // permit and leaves the approval path.
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
        if plan != row.plan_hash {
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
            args: row.args.clone(),
            pinned: request.pinned.clone(),
            created_at: row.created_at,
        };
        let built = build(&candidate, &rules, &*d.env, now).await;
        let (Verdict::Evaluate, Some(input)) = (built.verdict, built.request) else {
            return Err(withheld(PermitRefusal::InputsUnavailable));
        };
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
            safety_identity: None,
        })
        .map_err(|_| withheld(PermitRefusal::InputsUnavailable))?;
        Ok(PermitGrant {
            authorization: Authorization::policy(permit),
            decision: document,
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
