//! The durable verification: a `wake` effect that re-runs the incident's check and hands the
//! answer to the writer, which commits `Succeeded → Verified` (or `VerifyFailed`) together
//! with the effect's completion.
//!
//! The re-check is read-only, so running it twice is harmless; that is why it is retried and
//! reclaimed freely.

use std::sync::Arc;

use domovoi_core::ports::{ClaimToken, EffectRecord, LinkError};
use domovoi_core::{ActionId, ActionStatus, EffectId, SignalStatus, Verdict};
use serde_json::Value;
use tokio::sync::oneshot;

use super::Shared;
use super::executor::ask;
use super::recheck::RECHECK_DEADLINE;
use super::writer::WriteCommand;
use crate::effects::{EffectHandler, EffectOutcome};

/// Re-runs the check that raised the incident after a successful run.
pub(crate) struct VerifyWake {
    shared: Arc<Shared>,
}

impl VerifyWake {
    /// A handler over the action service's writer and dependencies.
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }
}

/// The action a wake payload names, or `None` when it is not a version 1 verify payload.
fn action_of(payload: &Value) -> Option<ActionId> {
    if payload.get("v")?.as_u64()? != 1 {
        return None;
    }
    if payload.get("purpose")?.as_str()? != "verify_action" {
        return None;
    }
    payload.get("action")?.as_str()?.parse().ok()
}

/// Marks the effect done without touching action state, for a wake that has nothing to verify.
async fn finish(shared: &Shared, effect: EffectId, claim: ClaimToken) -> EffectOutcome {
    let (reply, answer) = oneshot::channel();
    let command = WriteCommand::FinishEffect {
        effect,
        claim,
        reply,
    };
    if shared.writer.tx.send(command).await.is_err() {
        tracing::debug!(
            %effect,
            "the action service is closing; the wake is left for the next claim",
        );
        return EffectOutcome::Handled;
    }
    if let Ok(Err(err)) = answer.await {
        tracing::warn!(%err, %effect, "could not finish a wake effect");
    }
    EffectOutcome::Handled
}

#[async_trait::async_trait]
impl EffectHandler for VerifyWake {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &self.shared;
        let Some(action_id) = action_of(&effect.payload) else {
            return EffectOutcome::Dead {
                error: "unreadable wake payload".to_owned(),
            };
        };
        let action = match shared.deps.actions.get(action_id).await {
            Ok(Some(action)) => action,
            Ok(None) => {
                return EffectOutcome::Dead {
                    error: "the action no longer exists".to_owned(),
                };
            }
            Err(err) => {
                return EffectOutcome::Retry {
                    error: err.to_string(),
                };
            }
        };
        // Already verified, or never ran: there is nothing to verify and no state to change.
        if action.status() != &ActionStatus::Succeeded {
            return finish(shared, effect.id, claim).await;
        }
        let incident = match shared.deps.incidents.get(action.incident()).await {
            Ok(Some(incident)) => incident,
            Ok(None) => return finish(shared, effect.id, claim).await,
            Err(err) => {
                return EffectOutcome::Retry {
                    error: err.to_string(),
                };
            }
        };
        let now = shared.deps.clock.now();
        let deadline = now.checked_add(RECHECK_DEADLINE).unwrap_or(now);
        let recheck = shared
            .deps
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
                shared.deps.redactor.redact(&result.summary),
            ),
            Err(err) => {
                let transport = matches!(
                    err,
                    LinkError::NotConnected | LinkError::Timeout | LinkError::Transport(_)
                );
                if transport && effect.attempt < effect.max_attempts {
                    return EffectOutcome::Retry {
                        error: shared.deps.redactor.redact(&err.to_string()),
                    };
                }
                let reason = shared
                    .deps
                    .redactor
                    .redact(&format!("the re-check could not run: {err}"));
                (false, reason)
            }
        };
        let (reply, answer) = oneshot::channel();
        let verified = ask(
            shared,
            action.id(),
            "a re-check",
            WriteCommand::Verified {
                id: action.id(),
                passed,
                summary,
                effect: Some((effect.id, claim)),
                reply,
            },
            answer,
        )
        .await;
        if verified.is_some()
            && passed
            && let Err(err) = shared
                .deps
                .manager
                .set_verdict(incident.id(), Verdict::Fixed)
                .await
        {
            tracing::warn!(%err, "could not mark the incident fixed");
        }
        EffectOutcome::Handled
    }
}
