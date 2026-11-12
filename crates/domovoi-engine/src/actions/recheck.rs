//! Whether a finished run fixed what raised the incident: the re-check of the originating
//! check, and the `verified` command that carries its answer to the writer task (ADR 0019).
//! The verdict on the incident is set only once the writer has answered, so a run cannot
//! mark an incident fixed that the writer refused to record.

use domovoi_core::{ActionId, ActionRequest, CheckKey, Risk, SignalStatus, SkillSummary, Verdict};
use jiff::SignedDuration;
use tokio::sync::oneshot;

use super::Shared;
use super::executor::ask;
use super::writer::WriteCommand;

/// How long a verification re-check may take.
pub const RECHECK_DEADLINE: SignedDuration = SignedDuration::from_secs(60);

/// Queues the re-check and answers with the action as the writer left it: `Verified` or
/// `VerifyFailed`, or `None` when the writer refused or is closing.
async fn ask_verified(
    shared: &Shared,
    id: ActionId,
    passed: bool,
    summary: String,
) -> Option<ActionRequest> {
    let (reply, answer) = oneshot::channel();
    ask(
        shared,
        id,
        "a re-check",
        WriteCommand::Verified {
            id,
            passed,
            summary,
            reply,
        },
        answer,
    )
    .await
}

/// How long to wait before re-checking, or `None` when the action is not verified. A
/// skill that says how to verify itself wins; otherwise a write skill that applies to
/// the check that raised the incident waits the service's default.
fn verify_delay(shared: &Shared, skill: &SkillSummary, check: &CheckKey) -> Option<SignedDuration> {
    match &skill.verify {
        Some(spec) if spec.recheck => Some(spec.delay),
        Some(_) => None,
        None => (skill.risk == Risk::Write && skill.applies_to.contains(check))
            .then_some(shared.settings.verify_default_delay),
    }
}

/// The re-check of the originating check when the run succeeded: the skill's own
/// `verify` or the incident's check. The re-check runs here, but the transition it
/// decides is queued for the writer, which answers before the verdict is set.
pub(crate) async fn verify(shared: &Shared, action: &ActionRequest, skill: &SkillSummary) {
    let deps = &shared.deps;
    let Ok(Some(incident)) = deps.incidents.get(action.incident()).await else {
        return;
    };
    let Some(delay) = verify_delay(shared, skill, incident.check()) else {
        return;
    };
    let now = deps.clock.now();
    deps.clock
        .sleep_until(now.checked_add(delay).unwrap_or(now))
        .await;
    let deadline = deps
        .clock
        .now()
        .checked_add(RECHECK_DEADLINE)
        .unwrap_or(now);
    let recheck = deps
        .link
        .run_check(
            action.host(),
            incident.check().clone(),
            incident.target().cloned(),
            deadline,
        )
        .await;
    let (passed, summary) = match recheck {
        Ok(result) => (
            result.status == SignalStatus::Resolved,
            deps.redactor.redact(&result.summary),
        ),
        Err(err) => (false, format!("the re-check could not run: {err}")),
    };
    let Some(_action) = ask_verified(shared, action.id(), passed, summary).await else {
        return;
    };
    if passed
        && let Err(err) = deps
            .manager
            .set_verdict(action.incident(), Verdict::Fixed)
            .await
    {
        tracing::warn!(%err, "could not mark the incident fixed");
    }
}
