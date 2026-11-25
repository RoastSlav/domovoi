//! The autonomy commands: pause, resume, reset a rule's circuit breaker and resolve an unknown
//! outcome.
//!
//! Pausing narrows what the hub may do, so a session is enough. Resuming, resetting and resolving
//! widen it again, so each asks for the password first, through the same check and the same
//! login limiter as publishing a policy. The check is the first thing each does: a refused
//! password writes nothing, not even an audit row. Every change is one commit with its audit row.
//! The model has no tool that reaches any of these.

use std::sync::Arc;

use domovoi_core::ports::{
    AuditDraft, CommitError, OperationRepo, PermitRepo, PermitWrite, StateWrite, Transaction,
    UnknownResolution,
};
use domovoi_core::{Actor, ExecutionId, StepId, StepOutcome};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use super::Caller;
use super::policy::PolicyService;
use super::policy_types::{PolicyCommandError, StepUp};

/// The longest reason or note an owner may write.
const TEXT_MAX: usize = 280;
/// Audit kind of a pause.
pub const AUDIT_PAUSED: &str = "autonomy.paused";
/// Audit kind of a resume.
pub const AUDIT_RESUMED: &str = "autonomy.resumed";
/// Audit kind of a circuit-breaker reset.
pub const AUDIT_RULE_RESET: &str = "policy.rule_reset";
/// Audit kind of a resolved unknown outcome.
pub const AUDIT_UNKNOWN_RESOLVED: &str = "execution.unknown_resolved";

/// What the autonomy commands read and the breaker's settings.
#[derive(Clone)]
pub struct AutonomyDeps {
    /// The pause switch and the breaker's inputs.
    pub permits: Arc<dyn PermitRepo>,
    /// Stored operations, to check that a step has an unknown outcome.
    pub operations: Arc<dyn OperationRepo>,
    /// Failures inside the window that suspend a rule.
    pub circuit_failures: u32,
    /// The window failures are counted in.
    pub circuit_window: SignedDuration,
}

/// One rule whose breaker is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspendedRule {
    /// The rule.
    pub rule: String,
    /// How many failures are counted.
    pub failures: u32,
    /// When the earliest counted failure settled.
    pub since: Timestamp,
}

/// The autonomy switches as the owner sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutonomyView {
    /// Whether autonomy is paused.
    pub paused: bool,
    /// Who last changed the switch.
    pub paused_by: Option<String>,
    /// When it last changed.
    pub paused_at: Option<Timestamp>,
    /// Why it was paused.
    pub reason: Option<String>,
    /// Rules whose breaker is open.
    pub suspended_rules: Vec<SuspendedRule>,
}

fn bad(text: &str) -> PolicyCommandError {
    PolicyCommandError::BadRequest(text.to_owned())
}

/// `text` trimmed, 1 to 280 characters, or a refusal naming `what`.
fn bounded(text: &str, what: &str) -> Result<String, PolicyCommandError> {
    let text = text.trim();
    let length = text.chars().count();
    if length == 0 || length > TEXT_MAX {
        return Err(bad(&format!("{what} needs 1 to {TEXT_MAX} characters")));
    }
    Ok(text.to_owned())
}

impl PolicyService {
    /// The same service, able to pause, resume, reset a breaker and resolve unknown outcomes.
    #[must_use]
    pub fn with_autonomy(mut self, deps: AutonomyDeps) -> Self {
        self.autonomy = Some(deps);
        self
    }

    fn autonomy_deps(&self) -> Result<&AutonomyDeps, PolicyCommandError> {
        self.autonomy
            .as_ref()
            .ok_or(PolicyCommandError::Unavailable)
    }

    async fn commit_autonomy(&self, tx: Transaction) -> Result<(), PolicyCommandError> {
        self.uow
            .commit(tx)
            .await
            .map(|_| ())
            .map_err(|error| match error {
                CommitError::Constraint(text) => PolicyCommandError::Conflict(text),
                other => PolicyCommandError::Commit(other),
            })
    }

    /// The switches as they are now, with each suspended rule.
    pub async fn autonomy(&self) -> Result<AutonomyView, PolicyCommandError> {
        let deps = self.autonomy_deps()?;
        let now = self.clock.now();
        let since = now.checked_sub(deps.circuit_window).unwrap_or(now);
        let state = deps.permits.autonomy().await?;
        let failures = deps.permits.rule_failures(since).await?;
        Ok(AutonomyView {
            paused: state.paused,
            paused_by: state.changed_by,
            paused_at: state.changed_at,
            reason: state.reason,
            suspended_rules: failures
                .into_iter()
                .filter(|entry| entry.failures >= deps.circuit_failures)
                .map(|entry| SuspendedRule {
                    rule: entry.rule,
                    failures: entry.failures,
                    since: entry.since,
                })
                .collect(),
        })
    }

    /// Pauses autonomy: no new permit, and no dispatch of one already issued. It narrows what
    /// the hub may do, so a session is enough. Pausing while paused changes nothing.
    pub async fn pause_autonomy(
        &self,
        caller: &Caller,
        reason: &str,
    ) -> Result<(), PolicyCommandError> {
        let reason = bounded(reason, "the reason")?;
        let deps = self.autonomy_deps()?;
        if deps.permits.autonomy().await?.paused {
            return Ok(());
        }
        let now = self.clock.now();
        let tx = Transaction::new()
            .write(StateWrite::Permit(PermitWrite::Pause {
                by: caller.user().to_string(),
                reason: reason.clone(),
                at: now,
            }))
            .audit(AuditDraft::new(
                Actor::User(caller.user()),
                now,
                AUDIT_PAUSED.to_owned(),
                "autonomy".to_owned(),
                json!({ "reason": reason }),
            ));
        self.commit_autonomy(tx).await
    }

    /// Resumes autonomy. It widens what the hub may do, so it asks for the password first.
    pub async fn resume_autonomy(
        &self,
        caller: &Caller,
        step_up: StepUp<'_>,
    ) -> Result<(), PolicyCommandError> {
        self.step_up(caller, step_up).await?;
        let deps = self.autonomy_deps()?;
        if !deps.permits.autonomy().await?.paused {
            return Ok(());
        }
        let now = self.clock.now();
        let tx = Transaction::new()
            .write(StateWrite::Permit(PermitWrite::Resume {
                by: caller.user().to_string(),
                at: now,
            }))
            .audit(AuditDraft::new(
                Actor::User(caller.user()),
                now,
                AUDIT_RESUMED.to_owned(),
                "autonomy".to_owned(),
                json!({}),
            ));
        self.commit_autonomy(tx).await
    }

    /// Resets one rule's circuit breaker: failures before now stop counting. Asks for the
    /// password first.
    pub async fn reset_rule(
        &self,
        caller: &Caller,
        rule: &str,
        step_up: StepUp<'_>,
    ) -> Result<(), PolicyCommandError> {
        self.step_up(caller, step_up).await?;
        self.autonomy_deps()?;
        let valid = !rule.is_empty()
            && rule.len() <= 64
            && rule
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'));
        if !valid {
            return Err(bad("that is not a rule id"));
        }
        let now = self.clock.now();
        let tx = Transaction::new()
            .write(StateWrite::Permit(PermitWrite::ResetRule {
                rule: rule.to_owned(),
                by: caller.user().to_string(),
                at: now,
            }))
            .audit(AuditDraft::new(
                Actor::User(caller.user()),
                now,
                AUDIT_RULE_RESET.to_owned(),
                format!("rule:{rule}"),
                json!({ "rule": rule }),
            ));
        self.commit_autonomy(tx).await
    }

    /// Clears the unattended block of an `OutcomeUnknown` step: an owner has checked the host.
    /// A human approval was never blocked. Asks for the password first.
    pub async fn resolve_unknown(
        &self,
        caller: &Caller,
        step: StepId,
        note: &str,
        step_up: StepUp<'_>,
    ) -> Result<(), PolicyCommandError> {
        self.step_up(caller, step_up).await?;
        let note = bounded(note, "the note")?;
        let deps = self.autonomy_deps()?;
        let execution = ExecutionId::from_step(step);
        let row = deps
            .operations
            .execution(execution)
            .await?
            .ok_or(PolicyCommandError::NotFound)?;
        let snapshot = deps
            .operations
            .load(row.value.operation)
            .await?
            .ok_or(PolicyCommandError::NotFound)?;
        let outcome = snapshot.step(step).map(|record| record.progress.outcome);
        if outcome != Some(StepOutcome::OutcomeUnknown) {
            return Err(PolicyCommandError::Conflict(
                "this step does not have an unknown outcome".to_owned(),
            ));
        }
        let now = self.clock.now();
        let tx = Transaction::new()
            .write(StateWrite::Permit(PermitWrite::ResolveUnknown {
                execution,
                by: caller.user().to_string(),
                how: UnknownResolution::Owner,
                note: Some(note.clone()),
                at: now,
            }))
            .audit(AuditDraft::new(
                Actor::User(caller.user()),
                now,
                AUDIT_UNKNOWN_RESOLVED.to_owned(),
                format!("execution:{execution}"),
                json!({ "note": note }),
            ));
        self.commit_autonomy(tx).await
    }
}
