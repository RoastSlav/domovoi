//! `run_capability` for `firewall.block_source` and `firewall.list_blocks`: the checks the agent
//! owns, then the helper call, then the journal record.
//!
//! In this order, cheapest first, and nothing is journaled until the arguments parsed: fenced
//! journal (the caller), contract, version, whether the helper offers the `nft` group, the
//! unattended ceiling and the verified-safety rule (policy-labelled only), the presence of
//! `expected_safety`, expiry, arguments, the proof that a `confirm` or `rollback` belongs to a
//! block this agent applied, then acceptance (deduplication, the mutation limiter for `apply`
//! only, the durable record), the helper call, and the durable result.
//!
//! The helper recomputes every safety decision itself. The agent only refuses what it can know
//! alone, and refuses rather than clamps.

use domovoi_core::attest::ExpectedSafety;
use domovoi_core::containment::keys;
use domovoi_core::helper::HelperResponse;
use domovoi_proto::messages::{
    ActionState, AuthorizationKind, CapabilityOutcome, ErrorMessage, RefusalCode, Refused,
    RunCapability,
};
use domovoi_proto::{Message, Open};

use super::capability::{bad_request, unavailable};
use super::{RequestCtx, refused};
use crate::containment::args::{self, BlockArgs, Phase, ceiling_ask};
use crate::containment::backend::{call_for, map_answer};
use crate::containment::{BLOCK_SOURCE, LIST_BLOCKS};
use crate::helper_client::{FirewallCall, HelperClientError};
use crate::journal::{AcceptOutcome, AcceptedRequest};

/// A refusal whose code the wire enum does not name; it travels as an unknown `Open` code.
fn refused_open(code: &str, detail: impl Into<String>) -> Message {
    Message::Refused(Refused {
        code: Open::new(code, None),
        detail: detail.into(),
        retry_after: None,
        scope: None,
    })
}

/// The `active` count a `list_blocks` answer carries, or `None` when it carries none.
fn active_count(answer: Result<HelperResponse, HelperClientError>) -> Option<u32> {
    match answer {
        Ok(HelperResponse::Outputs { outputs }) => outputs.get(keys::OUT_ACTIVE)?.parse().ok(),
        _ => None,
    }
}

/// Serves a `run_capability` request for a containment contract.
pub async fn run_firewall(ctx: &RequestCtx, run: &RunCapability) -> Message {
    let contract = run.contract.as_str();
    let block_source = contract == BLOCK_SOURCE;
    // 2. Only version 1 exists.
    if run.version != 1 {
        return refused(
            "contract_version_unsupported",
            RefusalCode::ContractVersionUnsupported,
            format!("{contract} is only offered at version 1"),
        );
    }
    // 3. The owner's switch: the helper must offer the whole `nft` group right now.
    let groups = match ctx.helper.groups(&*ctx.clock).await {
        Ok(groups) => groups,
        Err(HelperClientError::Absent) => {
            return refused(
                "contract_unknown",
                RefusalCode::ContractUnknown,
                format!("this host has no helper, so {contract} is unknown"),
            );
        }
        Err(_) => return unavailable("the helper cannot be reached"),
    };
    if groups.nft_ops().is_empty() {
        return refused(
            "contract_unknown",
            RefusalCode::ContractUnknown,
            "the helper does not offer the nft operations",
        );
    }
    let boot = crate::boot::boot_snapshot(&*ctx.fs).await.ok();
    if !block_source {
        // The refresh is owner-initiated: a policy label never reads blocks.
        if run.authorization_kind == AuthorizationKind::Policy {
            return refused(
                "allow_risk_denied",
                RefusalCode::AllowRiskDenied,
                "listing blocks needs a human authorization",
            );
        }
    } else {
        // 4. A policy label needs a verified identity and is checked against the ceiling.
        if run.authorization_kind == AuthorizationKind::Policy {
            if matches!(run.expected_safety, Some(ExpectedSafety::Unverified { .. })) {
                return refused(
                    "policy_requires_verified_safety",
                    RefusalCode::PolicyRequiresVerifiedSafety,
                    "an unattended block needs a verified safety identity",
                );
            }
            if let Some(refusal) = check_ceiling(ctx, run, boot.as_ref()).await {
                return refusal;
            }
        }
        // 5. Every phase carries what the approval bound; the helper compares it.
        if run.expected_safety.is_none() {
            return refused_open(
                "malformed_safety",
                "a block request carries expected_safety in every phase",
            );
        }
        // 6. And an authorization still in date.
        if run.expires_at <= ctx.clock.now() {
            return refused(
                "authorization_expired",
                RefusalCode::AuthorizationExpired,
                "the authorization for this block has expired",
            );
        }
    }
    // 7. Arguments, before anything is journaled.
    let parsed = if block_source {
        match args::parse(run) {
            Ok(parsed) => Some(parsed),
            Err(message) => return bad_request(message),
        }
    } else if run.args.is_empty() {
        None
    } else {
        return bad_request("firewall.list_blocks takes no arguments");
    };
    // 8. No journal or no boot snapshot: nothing is accepted.
    let (Some(journal), Some(snapshot)) = (ctx.journal.clone(), boot) else {
        return unavailable("the action journal or the boot identity is not available");
    };
    // 9. A confirm or rollback must belong to a block whose apply finished here.
    if let Some(parsed) = &parsed
        && parsed.phase != Phase::Apply
        && !journal
            .finished_ok(BLOCK_SOURCE, parsed.block)
            .unwrap_or(false)
    {
        return refused_open(
            "unknown_block",
            "this agent has no finished apply for that block",
        );
    }
    // 10. The call the helper will get.
    let call = match &parsed {
        Some(parsed) => match call_for(parsed, run.expected_safety.clone()) {
            Some(call) => call,
            None => return bad_request("the source is not a shape the helper accepts"),
        },
        None => FirewallCall::List,
    };
    // 11. The agent's own active limit, from the helper's own count (policy applies only).
    if let Some(refusal) = check_active(ctx, run, parsed.as_ref()).await {
        return refusal;
    }
    // 12. Acceptance: deduplication, the mutation limiter for `apply` only (a block's other
    // phases belong to an accepted block), and the durable `accepted` record.
    let plan_hash = run.plan_hash.to_string();
    let request = AcceptedRequest {
        execution_id: run.execution_id,
        contract,
        version: run.version,
        target: run.target.as_str(),
        plan_hash: &plan_hash,
        authorization_kind: run.authorization_kind,
        mutating: parsed.as_ref().is_some_and(|p| p.phase == Phase::Apply),
    };
    match journal.accept(
        &request,
        ctx.clock.now(),
        &snapshot.boot_id.to_string(),
        snapshot.monotonic_ns,
    ) {
        Ok(AcceptOutcome::Accepted) => {}
        Ok(AcceptOutcome::Refused(refusal)) => return Message::Refused(*refusal),
        Ok(AcceptOutcome::Duplicate(answer)) => {
            return match answer.state {
                ActionState::Finished { result } => Message::CapabilityResult(result),
                ActionState::Running => Message::Error(ErrorMessage {
                    code: "in_progress".to_owned(),
                    message: "this execution is still running".to_owned(),
                }),
                ActionState::NotFound | ActionState::OutcomeUnknown { .. } => {
                    Message::Error(ErrorMessage {
                        code: "outcome_unknown".to_owned(),
                        message: "this execution id was used before and its outcome is unknown; read the blocks and retry with a new id".to_owned(),
                    })
                }
            };
        }
        Err(error) => return unavailable(&format!("the action journal refused: {error}")),
    }
    // 13. The helper does the work; the agent holds no process of its own, so pid 0.
    if let Err(error) = journal.started(run.execution_id, 0, 0) {
        return unavailable(&format!("the action journal refused: {error}"));
    }
    let mut result = map_answer(ctx.helper.firewall(&call, &*ctx.clock).await);
    if let CapabilityOutcome::Failed { code, message } = result.outcome {
        result.outcome = CapabilityOutcome::Failed {
            code,
            message: ctx.redactor.redact(&message),
        };
    }
    // 14. Durable result; a failed append is logged and the real result still goes out.
    if let Err(error) = journal.finished(run.execution_id, result.clone()) {
        tracing::error!(%error, execution = %run.execution_id, "the journal could not record a containment result");
    }
    Message::CapabilityResult(result)
}

/// The ceiling step for a policy-labelled block, or `None` when it passes.
async fn check_ceiling(
    ctx: &RequestCtx,
    run: &RunCapability,
    boot: Option<&crate::boot::BootSnapshot>,
) -> Option<Message> {
    let (Some(ceiling), Some(snapshot), Some(journal)) = (&ctx.ceiling, boot, &ctx.journal) else {
        return Some(refused(
            "ceiling_exceeded",
            RefusalCode::CeilingExceeded,
            "ceiling: this host has no unattended ceiling",
        ));
    };
    // Only an `apply` spends the hour.
    let applying = run.args.get(keys::ARG_PHASE).map(String::as_str) == Some(keys::PHASE_APPLY);
    let accepted = if applying {
        Some(
            journal
                .policy_accepted_mutating(
                    BLOCK_SOURCE,
                    snapshot.monotonic_ns,
                    &snapshot.boot_id.to_string(),
                )
                .unwrap_or(u32::MAX),
        )
    } else {
        None
    };
    ceiling
        .check_firewall(
            BLOCK_SOURCE,
            run.target.as_str(),
            &ceiling_ask(run),
            accepted,
        )
        .err()
        .map(|step| {
            refused(
                "ceiling_exceeded",
                RefusalCode::CeilingExceeded,
                step.detail(),
            )
        })
}

/// The agent's `max_active` for a policy-labelled `apply`, against the helper's own count.
async fn check_active(
    ctx: &RequestCtx,
    run: &RunCapability,
    parsed: Option<&BlockArgs>,
) -> Option<Message> {
    if run.authorization_kind != AuthorizationKind::Policy
        || parsed.is_none_or(|p| p.phase != Phase::Apply)
    {
        return None;
    }
    let limit = ctx.ceiling.as_ref()?.max_active(BLOCK_SOURCE)?;
    let answer = ctx.helper.firewall(&FirewallCall::List, &*ctx.clock).await;
    let Some(active) = active_count(answer) else {
        return Some(unavailable("the helper could not count the active blocks"));
    };
    (active >= limit).then(|| {
        refused(
            "ceiling_exceeded",
            RefusalCode::CeilingExceeded,
            crate::unattended::CeilingStep::ActiveSpent.detail(),
        )
    })
}

/// Whether `contract` is one of the containment contracts.
#[must_use]
pub fn is_firewall_contract(contract: &str) -> bool {
    contract == BLOCK_SOURCE || contract == LIST_BLOCKS
}
