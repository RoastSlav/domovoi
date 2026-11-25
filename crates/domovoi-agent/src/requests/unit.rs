//! `run_capability` for `unit.identity` and `unit.restart`: the checks the agent owns, then
//! the helper call, then the journal record.
//!
//! In this order, cheapest first, and nothing is journaled until the arguments parsed:
//! fenced journal, contract and version, whether the helper offers the operation, `allow_risk`
//! (restart only), the unattended ceiling (policy-labelled only), expiry, arguments, the
//! journal and boot snapshot, acceptance (deduplication, the mutation limiter, the durable
//! record), the helper call, and the durable result. The agent does not interpret the
//! identity: it relays outputs unchanged.

use domovoi_core::helper::{HelperResponse, UnavailableReason};
use domovoi_core::skill::Risk;
use domovoi_core::unit::{IdentityHash, UnitCall, UnitId, UnitName, keys};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    ActionState, AuthorizationKind, CapabilityFailureCode, CapabilityOutcome, CapabilityResult,
    ErrorMessage, RefusalCode, RunCapability,
};

use super::capability::{bad_request, failed, unavailable};
use super::{RequestCtx, refused};
use crate::helper_client::{GroupsInfo, HelperClientError};
use crate::journal::{AcceptOutcome, AcceptedRequest};

/// The contract of a unit identity read.
pub const IDENTITY: &str = "unit.identity";
/// The contract of a unit restart.
pub const RESTART: &str = "unit.restart";

/// Whether `contract` is one of the unit contracts this module serves.
#[must_use]
pub fn is_unit_contract(contract: &str) -> bool {
    contract == IDENTITY || contract == RESTART
}

/// The call the arguments describe, or a sentence for `bad_request`.
fn parse_call(run: &RunCapability) -> Result<UnitCall, String> {
    let args = &run.args;
    let only =
        |keys: &[&str]| args.len() == keys.len() && keys.iter().all(|k| args.contains_key(*k));
    if run.contract == IDENTITY {
        if !only(&[keys::ARG_UNIT]) {
            return Err("unit.identity takes exactly one argument, `unit`".to_owned());
        }
        let name = UnitName::parse(&args[keys::ARG_UNIT]).map_err(|e| e.to_string())?;
        return Ok(UnitCall::Identity { name });
    }
    if !only(&[keys::ARG_UNIT, keys::ARG_IDENTITY_HASH]) {
        return Err("unit.restart takes exactly `unit` and `identity_hash`".to_owned());
    }
    let id = UnitId::parse(&args[keys::ARG_UNIT]).map_err(|e| e.to_string())?;
    if run.target.as_str() != id.as_str() {
        return Err("the target must be the canonical unit id".to_owned());
    }
    let hash: IdentityHash = args[keys::ARG_IDENTITY_HASH]
        .parse()
        .map_err(|_| "identity_hash must be 64 lowercase hex characters".to_owned())?;
    Ok(UnitCall::Restart {
        id,
        identity_hash: hash,
    })
}

/// What the helper's answer means as a capability result. Helper tokens come back as
/// `Failed { Refused, "<token>: <detail>" }` (the convention 41 and 42 use); the tier gate's two
/// reasons become the tokens the hub and the card show.
fn map_answer(
    answer: Result<HelperResponse, HelperClientError>,
    groups: &GroupsInfo,
) -> CapabilityResult {
    match answer {
        Ok(HelperResponse::Outputs { mut outputs }) => {
            outputs.insert(
                keys::OUT_PLATFORM_TIER.to_owned(),
                format!("{:?}", groups.platform_tier).to_lowercase(),
            );
            outputs.insert(
                keys::OUT_WRITE_OPT_IN.to_owned(),
                groups.write_opt_in.to_string(),
            );
            CapabilityResult {
                outcome: CapabilityOutcome::Succeeded { outputs },
            }
        }
        Ok(HelperResponse::UnitRefused { token, detail }) => {
            let message = match detail {
                Some(detail) => format!("{token}: {detail}"),
                None => token,
            };
            failed(CapabilityFailureCode::Refused, message)
        }
        Ok(HelperResponse::Unavailable { reason, .. }) => match reason {
            UnavailableReason::WriteGroupNeedsOptIn => failed(
                CapabilityFailureCode::Refused,
                "platform_untested".to_owned(),
            ),
            UnavailableReason::PlatformBlocked => failed(
                CapabilityFailureCode::Refused,
                "platform_blocked".to_owned(),
            ),
            UnavailableReason::GroupDisabled | UnavailableReason::NotImplemented => failed(
                CapabilityFailureCode::HelperUnavailable,
                "the helper does not offer this operation".to_owned(),
            ),
        },
        Ok(HelperResponse::Refused { token }) => {
            failed(CapabilityFailureCode::Refused, token.as_str().to_owned())
        }
        Ok(_) => failed(
            CapabilityFailureCode::Refused,
            "the helper answered something unexpected".to_owned(),
        ),
        Err(error) => failed(
            CapabilityFailureCode::HelperUnavailable,
            error.relay_code().to_owned(),
        ),
    }
}

/// Serves a `run_capability` request for a unit contract.
pub async fn run_unit(ctx: &RequestCtx, run: &RunCapability) -> Message {
    let contract = run.contract.as_str();
    let restart = contract == RESTART;
    // 2. Only version 1 exists.
    if run.version != 1 {
        return refused(
            "contract_version_unsupported",
            RefusalCode::ContractVersionUnsupported,
            format!("{contract} is only offered at version 1"),
        );
    }
    // 3. The helper must offer the operation right now.
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
    let op = if restart {
        "unit.restart"
    } else {
        "unit.identity"
    };
    if !groups.unit_ops().contains(&op) {
        return refused(
            "contract_unknown",
            RefusalCode::ContractUnknown,
            format!("the helper does not offer {op}"),
        );
    }
    let boot = crate::boot::boot_snapshot(&*ctx.fs).await.ok();
    if restart {
        // 4. `allow_risk` must contain write.
        if !ctx.allow_risk.contains(&Risk::Write) {
            return refused(
                "allow_risk_denied",
                RefusalCode::AllowRiskDenied,
                "this agent's policy does not allow write capabilities",
            );
        }
        // 5. A policy label is checked against the ceiling, step by step.
        if run.authorization_kind == AuthorizationKind::Policy {
            let (Some(ceiling), Some(snapshot), Some(journal)) =
                (&ctx.ceiling, &boot, &ctx.journal)
            else {
                return refused(
                    "ceiling_exceeded",
                    RefusalCode::CeilingExceeded,
                    "ceiling: this host has no unattended ceiling",
                );
            };
            let accepted = journal
                .policy_accepted(
                    contract,
                    snapshot.monotonic_ns,
                    &snapshot.boot_id.to_string(),
                )
                .unwrap_or(u32::MAX);
            if let Err(step) = ceiling.check(contract, run.target.as_str(), accepted) {
                return refused(
                    "ceiling_exceeded",
                    RefusalCode::CeilingExceeded,
                    step.detail(),
                );
            }
        }
        // 6. And an authorization still in date.
        if run.expires_at <= ctx.clock.now() {
            return refused(
                "authorization_expired",
                RefusalCode::AuthorizationExpired,
                "the authorization for this restart has expired",
            );
        }
    }
    // 7. Arguments, before anything is journaled.
    let call = match parse_call(run) {
        Ok(call) => call,
        Err(message) => return bad_request(message),
    };
    // 8. No journal or no boot snapshot: nothing is accepted.
    let (Some(journal), Some(snapshot)) = (ctx.journal.clone(), boot) else {
        return unavailable("the action journal or the boot identity is not available");
    };
    // 9. Acceptance: deduplication, the mutation limiter (human and policy alike) and the
    // durable `accepted` record.
    let plan_hash = run.plan_hash.to_string();
    let request = AcceptedRequest {
        execution_id: run.execution_id,
        contract,
        version: run.version,
        target: run.target.as_str(),
        plan_hash: &plan_hash,
        authorization_kind: run.authorization_kind,
        mutating: restart,
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
                        message: "this execution id was used before and its outcome is unknown; check the host and retry with a new id".to_owned(),
                    })
                }
            };
        }
        Err(error) => return unavailable(&format!("the action journal refused: {error}")),
    }
    // 10. The helper does the work; the agent holds no process of its own, so pid 0.
    if let Err(error) = journal.started(run.execution_id, 0, 0) {
        return unavailable(&format!("the action journal refused: {error}"));
    }
    let answer = ctx.helper.unit(&call, &*ctx.clock).await;
    let mut result = map_answer(answer, &groups);
    if let CapabilityOutcome::Failed { code, message } = result.outcome {
        result.outcome = CapabilityOutcome::Failed {
            code,
            message: ctx.redactor.redact(&message),
        };
    }
    // 11. Durable result; a failed append is logged and the real result still goes out.
    if let Err(error) = journal.finished(run.execution_id, result.clone()) {
        tracing::error!(%error, execution = %run.execution_id, "the journal could not record a unit result");
    }
    Message::CapabilityResult(result)
}
