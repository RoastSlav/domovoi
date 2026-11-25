//! The dispatch of an authorized `firewall.block_source` step: the operation service's one
//! containment dispatch, a sibling of `execute_unit`.
//!
//! Before anything is sent it checks what only the hub can check (the window, the loop guard, for
//! a permit the pause switch and the unknown-outcome block, passed drills), assesses the helper's
//! safety again and recomputes the plan hash, so a change of the verified identity since approval
//! is a refusal and never a block. The phases themselves are run by
//! [`crate::containment::phases::execute_containment`].

use domovoi_core::action::{AuthorizationKind, ContainmentPlan};
use domovoi_core::attest::Authority;
use domovoi_core::capability::TargetQuery;
use domovoi_core::containment::{Net, keys};
use domovoi_core::ports::AgentSnapshot;
use domovoi_core::{ExecutionRecord, StepOutcome};
use jiff::SignedDuration;

use super::effects::{Ending, settle};
use super::execute::commit_dispatch;
use super::writer::{Held, OpShared, queue_reconcile};
use crate::containment::input::{AssessError, assess};
use crate::containment::phases::{ContainEnd, Prepared, execute_containment};
use crate::containment::{AuthorizedContainment, MIN_WINDOW, STEP_ONE_SECS};
use crate::operations::{AuthorizedStep, ExecuteError};

/// The longest block a request may ask for: a year, which `nft` accepts.
const MAX_TTL_S: i64 = 31_536_000;

fn log(result: Result<(), super::OperationError>, row: &ExecutionRecord) {
    if let Err(err) = result {
        tracing::debug!(%err, step = %row.step, "the result of a containment was not recorded");
    }
}

async fn refuse(
    shared: &OpShared,
    held: Held,
    row: &ExecutionRecord,
    reason: String,
) -> ExecuteError {
    let ending = Ending::new(row.operation, row.step, StepOutcome::Refused)
        .because(reason.clone())
        .holding(held);
    log(settle(shared, ending).await, row);
    ExecuteError::Refused(reason)
}

/// The network and the TTL a containment step asks for, from its stored arguments.
pub(crate) fn parse_request(
    args: &std::collections::BTreeMap<String, String>,
) -> Result<(Net, SignedDuration), String> {
    let source = args
        .get(keys::ARG_SOURCE)
        .ok_or_else(|| "a containment step names its source".to_owned())?;
    let net = Net::parse(source).map_err(|err| format!("invalid_target: {err}"))?;
    if net.canonical() != *source {
        return Err("invalid_target: the source is not in canonical form".to_owned());
    }
    let ttl: i64 = args
        .get(keys::ARG_TTL_S)
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| "a containment step names its ttl_s".to_owned())?;
    if ttl <= STEP_ONE_SECS {
        return Err("ttl_too_short".to_owned());
    }
    if ttl > MAX_TTL_S {
        return Err("ttl_above_maximum".to_owned());
    }
    Ok((net, SignedDuration::from_secs(ttl)))
}

/// Sends one authorized `firewall.block_source` to its host's agent, or settles the step without
/// sending.
pub(super) async fn execute_containment_step(
    shared: &OpShared,
    held: Held,
    authorized: AuthorizedStep,
    session: AgentSnapshot,
) -> Result<(), ExecuteError> {
    let authorized = AuthorizedContainment::of(authorized)?;
    let row = authorized.execution();
    let (operation, step, host) = (row.operation, row.step, row.host);
    let deps = &shared.deps;
    let Some(driver) = &deps.contain else {
        let reason = "containment is not set up on this hub".to_owned();
        return Err(refuse(shared, held, row, reason).await);
    };
    let now = deps.clock.now();
    if now >= row.expires_at {
        return Err(ExecuteError::Expired);
    }
    // The block's three phases must fit inside the authorization window.
    let window = row.expires_at.duration_since(now);
    if window < MIN_WINDOW {
        let reason = "approval_window_too_short".to_owned();
        return Err(refuse(shared, held, row, reason).await);
    }
    let gate = shared.dispatch_gate(host).await;
    let since = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let recent = deps
        .operations
        .dispatched_since(host, &row.contract, since)
        .await
        .map_err(|err| {
            tracing::warn!(%err, %step, "could not count the recent dispatches; not dispatching");
            ExecuteError::NotDispatched
        })?;
    let limit = shared.settings.loop_guard_per_hour;
    if recent >= limit {
        let reason = format!(
            "loop guard: {} already ran {limit} times on this host in the last hour",
            row.contract
        );
        return Err(refuse(shared, held, row, reason).await);
    }
    let policy = row.permit().is_some();
    if let Some(stored) = row.permit() {
        if now >= stored.permit().expires_at() {
            let ending = Ending::new(operation, step, StepOutcome::NotRun)
                .because("authorization_expired")
                .holding(held);
            log(settle(shared, ending).await, row);
            return Ok(());
        }
        let Some(autonomy) = &deps.autonomy else {
            let reason = "unattended containment is not set up on this hub".to_owned();
            return Err(refuse(shared, held, row, reason).await);
        };
        let state = autonomy.permits.autonomy().await.map_err(|err| {
            tracing::warn!(%err, %step, "could not read the pause switch; not dispatching");
            ExecuteError::NotDispatched
        })?;
        if state.paused {
            let ending = Ending::new(operation, step, StepOutcome::NotRun)
                .because("autonomy_paused")
                .holding(held);
            log(settle(shared, ending).await, row);
            return Ok(());
        }
        if let Some(target) = &row.target {
            let unknown = autonomy
                .permits
                .unresolved_unknown(host, &row.contract, target)
                .await
                .map_err(|err| {
                    tracing::warn!(%err, %step, "could not read unknown outcomes; not dispatching");
                    ExecuteError::NotDispatched
                })?;
            if unknown.is_some() {
                return Err(refuse(shared, held, row, "unresolved_unknown".to_owned()).await);
            }
        }
        if !driver.drills_passed(host) {
            return Err(refuse(shared, held, row, "drills_not_passed".to_owned()).await);
        }
    }
    let (net, ttl) = match parse_request(&row.args) {
        Ok(parsed) => parsed,
        Err(reason) => return Err(refuse(shared, held, row, reason).await),
    };
    // Assess again and recompute: a change since approval is a refusal, not a block.
    let cd = driver.deps();
    let authority = if policy {
        Authority::Policy
    } else {
        Authority::Human
    };
    let assessment = match assess(&*cd.trust, &*cd.paths, host, authority, now).await {
        Ok(assessment) => assessment,
        Err(AssessError::PolicyUnverified(reason)) => {
            let reason = format!("attestation: {}", reason.token());
            return Err(refuse(shared, held, row, reason).await);
        }
        Err(AssessError::Unreadable(_)) => {
            tracing::warn!(%step, "could not assess the helper; not dispatching");
            return Err(ExecuteError::NotDispatched);
        }
    };
    let resolved = match cd
        .resolver
        .resolve(TargetQuery::NetworkSource { host, source: net })
        .await
    {
        Ok(resolved) => resolved,
        Err(_) => {
            let reason = "the host can no longer be resolved".to_owned();
            return Err(refuse(shared, held, row, reason).await);
        }
    };
    let plan =
        ContainmentPlan::new(&net, ttl, host, &resolved.host_key, &assessment.assurance).hash();
    let epoch = commit_dispatch(shared, held, row, plan, &session).await?;
    if !super::execute::still_dispatched(shared, row, epoch).await {
        return Err(ExecuteError::NotDispatched);
    }
    let kind = row
        .authorization_kind()
        .ok_or(ExecuteError::NotAuthorized)?;
    let expires_at = match row.permit() {
        Some(stored) if kind == AuthorizationKind::Policy => {
            stored.permit().expires_at().min(row.expires_at)
        }
        _ => row.expires_at,
    };
    let prepared = Prepared {
        net,
        ttl,
        plan_hash: plan,
        assessment,
        authorization: kind,
        expires_at,
    };
    let end = execute_containment(cd, &authorized, &prepared, session.session, epoch).await;
    drop(gate);
    match end {
        ContainEnd::Settled {
            outcome,
            reason,
            output,
        } => {
            let mut ending = Ending::new(operation, step, outcome);
            ending.reason = reason;
            ending.output = output.map(|text| deps.redactor.redact(&text));
            log(settle(shared, ending).await, row);
        }
        ContainEnd::Unknown => {
            // A phase was handed over and nothing proves its result: the step stays dispatched
            // and the block settles from the agent's journal when the host reconnects.
            tracing::warn!(%step, "a containment phase's result is unavailable; it stays dispatched");
            if deps.journal.epoch_of(host) == Some(epoch)
                && let Err(err) = queue_reconcile(shared, operation, step).await
            {
                tracing::warn!(%err, %step, "could not queue a reconcile");
            }
        }
    }
    Ok(())
}
