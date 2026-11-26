//! The journal-backed path of `run_skill` for skills that change the host.
//!
//! A skill whose risk is not `read` is accepted, started and finished through the action
//! journal exactly like a capability: `accepted` is durable before anything runs (and the
//! mutation limiter applies), `started` before the runner is asked, `finished` before the
//! reply goes out. A lost reply is then settled by the hub from the journal, and a crash in
//! the middle of a run reads as "outcome unknown" rather than "never ran".
//!
//! The runner's process is not visible to the agent, so `started` records pid 0, as the unit
//! restart does. Standard output stays in the live reply; the journal keeps the exit status
//! and the run time only.

use std::collections::BTreeMap;

use domovoi_core::{ContractId, SkillName, ValidatedArgs};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    ActionState, AuthorizationKind, CapabilityFailureCode, CapabilityOutcome, CapabilityResult,
    ErrorMessage, RunSkill, SkillFailureCode, SkillOutcome, SkillResult,
};
use domovoi_runner::LoadedSkill;

use super::RequestCtx;
use super::capability::failed;
use super::skills::{fail, failure_message, redacted};
use crate::journal::{AcceptOutcome, AcceptedRequest};
use crate::skill_runner::RouteError;

/// The contract id the journal and the mutation limiter use for a skill: `skill.<name>`.
fn contract_of(name: &SkillName) -> String {
    ContractId::skill(name).as_str().to_owned()
}

fn skill_reply(outcome: SkillOutcome) -> Message {
    Message::SkillResult(SkillResult { outcome })
}

fn error(code: &str, message: &str) -> Message {
    Message::Error(ErrorMessage {
        code: code.to_owned(),
        message: message.to_owned(),
    })
}

/// What the journal keeps of a finished run.
fn journal_result(outcome: &SkillOutcome) -> CapabilityResult {
    match outcome {
        SkillOutcome::Ok(out) if out.exit_code == Some(0) => CapabilityResult {
            outcome: CapabilityOutcome::Succeeded {
                outputs: BTreeMap::from([
                    ("exit_code".to_owned(), "0".to_owned()),
                    ("truncated".to_owned(), out.truncated.to_string()),
                    ("duration_ms".to_owned(), out.duration_ms.to_string()),
                ]),
            },
        },
        SkillOutcome::Ok(out) => failed(
            CapabilityFailureCode::ExitedNonZero,
            match out.exit_code {
                Some(code) => format!("the skill exited with status {code}"),
                None => "the skill was killed by a signal".to_owned(),
            },
        ),
        SkillOutcome::Err(f) => failed(
            match f.code {
                SkillFailureCode::Timeout => CapabilityFailureCode::TimedOut,
                SkillFailureCode::Exec => CapabilityFailureCode::SpawnFailed,
                _ => CapabilityFailureCode::Refused,
            },
            f.message.clone(),
        ),
    }
}

/// Runs an admitted skill that changes the host, with a durable record around it.
pub(super) async fn run_journaled(
    ctx: &RequestCtx,
    run: &RunSkill,
    skill: &LoadedSkill,
    validated: &ValidatedArgs,
) -> Message {
    let name = run.skill.as_str();
    let (Some(journal), Some(snapshot)) = (
        ctx.journal.clone(),
        crate::boot::boot_snapshot(&*ctx.fs).await.ok(),
    ) else {
        tracing::warn!(skill = name, "refused a mutating skill: no action journal");
        return skill_reply(fail(
            SkillFailureCode::Exec,
            format!(
                "{name} changes the host and needs the action journal, which this agent does not \
                 have; run `domovoi agent journal init` and restart the agent"
            ),
        ));
    };
    let contract = contract_of(&run.skill);
    let target = format!("skill:{name}");
    let plan_hash = run.plan_hash.to_string();
    let execution_id = run.action_id.as_uuid();
    let request = AcceptedRequest {
        execution_id,
        contract: &contract,
        version: 1,
        target: &target,
        plan_hash: &plan_hash,
        authorization_kind: AuthorizationKind::Human,
        mutating: true,
    };
    match journal.accept(
        &request,
        ctx.clock.now(),
        &snapshot.boot_id.to_string(),
        snapshot.monotonic_ns,
    ) {
        Ok(AcceptOutcome::Accepted) => {}
        Ok(AcceptOutcome::Refused(refusal)) => return Message::Refused(*refusal),
        Ok(AcceptOutcome::Duplicate(answer)) => return duplicate(answer.state),
        Err(err) => {
            return skill_reply(fail(
                SkillFailureCode::Exec,
                format!("the action journal refused: {err}"),
            ));
        }
    }
    if let Err(err) = journal.started(execution_id, 0, 0) {
        return skill_reply(fail(
            SkillFailureCode::Exec,
            format!("the action journal refused: {err}"),
        ));
    }
    tracing::info!(skill = name, action = %run.action_id, "running an approved skill");
    let outcome = match ctx.skills.run(skill, &ctx.catalog, validated).await {
        Ok(output) => {
            tracing::info!(skill = name, exit_code = ?output.exit_code, "skill finished");
            SkillOutcome::Ok(redacted(ctx, output).into())
        }
        Err(RouteError::TimedOut(_)) => {
            tracing::warn!(skill = name, "skill timed out");
            fail(SkillFailureCode::Timeout, format!("{name} timed out"))
        }
        Err(err) => {
            tracing::warn!(skill = name, %err, "skill could not run");
            fail(SkillFailureCode::Exec, failure_message(name, &err))
        }
    };
    if let Err(err) = journal.finished(execution_id, journal_result(&outcome)) {
        tracing::error!(%err, execution = %run.action_id, "the journal could not record a skill result");
    }
    skill_reply(outcome)
}

fn duplicate(state: ActionState) -> Message {
    match state {
        ActionState::Running => error("in_progress", "this execution is still running"),
        ActionState::Finished { .. } => error(
            "already_finished",
            "this execution id was already used; its result is in the journal",
        ),
        ActionState::NotFound | ActionState::OutcomeUnknown { .. } => error(
            "outcome_unknown",
            "this execution id was used before and its outcome is unknown; check the host and retry with a new id",
        ),
    }
}
