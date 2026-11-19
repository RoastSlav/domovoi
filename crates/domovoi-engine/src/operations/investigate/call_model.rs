//! `CallModel`: the provider chain, the tool loop and the report parse, in one reasoning turn.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{FinishOutcome, StepOutcome, StepUsage};
use serde_json::json;

use super::DRAFT_ATTEMPTS;
use super::plumbing::{
    claim, conclude, end_operation, incident_of, latest_turn, runtime, short, step_of,
};
use super::turn_output::{TurnKind, TurnOutput};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::effects::{Address, address, after, finish, load, spec, unreadable};
use crate::operations::writer::{Conclusion, OpShared};
use crate::triage::agent::RunUsage;
use crate::triage::run::{failed, generate, succeeded};

/// The handler of `call_model` effects.
pub(super) struct CallModel {
    shared: Arc<OpShared>,
}

impl CallModel {
    /// A handler over the operation service.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

/// The usage a turn stores: nothing when no response came, and no cost when any response had
/// no price, so a price is never made up.
fn step_usage(usage: RunUsage) -> Option<StepUsage> {
    let answered = usage.input_tokens > 0 || usage.output_tokens > 0 || usage.cost_micros.is_some();
    answered.then(|| StepUsage {
        input_tokens: u32::try_from(usage.input_tokens).unwrap_or(u32::MAX),
        output_tokens: u32::try_from(usage.output_tokens).unwrap_or(u32::MAX),
        cost_micros: if usage.any_unpriced {
            None
        } else {
            usage.cost_micros
        },
    })
}

#[async_trait]
impl EffectHandler for CallModel {
    async fn handle(&self, effect: &EffectRecord, token: ClaimToken) -> EffectOutcome {
        let shared = &self.shared;
        let held = claim(effect, token);
        let (Some(Address { operation, step }), Some(bundle), Some(runtime)) = (
            address(effect),
            effect.payload.get("context").and_then(|c| c.as_str()),
            runtime(shared),
        ) else {
            return unreadable();
        };
        let Some(turn) = step else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        let running = step_of(&snapshot, turn)
            .is_some_and(|record| record.progress.outcome == StepOutcome::Running);
        if snapshot.operation.status.is_terminal() || !running {
            return finish(shared, held).await;
        }
        let incident = match incident_of(&runtime, &snapshot).await {
            Ok(Some(incident)) => incident,
            Ok(None) => {
                return end_operation(
                    shared,
                    held,
                    operation,
                    FinishOutcome::Failed,
                    "the incident no longer exists",
                )
                .await;
            }
            Err(done) => return done,
        };
        let triage = &runtime.triage;
        let skills = triage.deps.capabilities.enabled_skills(incident.host());
        // A provider failure is a result, never a retry: a retry would spend another slot of
        // the hour.
        let generated = generate(triage, &incident, &skills, bundle).await;
        let usage = step_usage(generated.usage);
        let ended = match generated.result {
            Ok(report) => {
                succeeded(triage).await;
                let summary = short(&report.headline);
                let mut output = TurnOutput::of(TurnKind::Report);
                output.report = Some(report);
                Conclusion {
                    step: turn,
                    outcome: StepOutcome::Succeeded,
                    summary: Some(summary),
                    reason: None,
                    output: Some(output.to_json()),
                    usage,
                }
            }
            Err(reason) => {
                tracing::warn!(incident = %incident.id(), reason = reason.as_str(), "triage failed");
                failed(triage, &incident, &reason).await;
                let mut output = TurnOutput::of(TurnKind::Failed);
                output.reason = Some(reason.clone());
                Conclusion {
                    step: turn,
                    outcome: StepOutcome::Failed,
                    summary: None,
                    reason: Some(short(&reason)),
                    output: Some(output.to_json()),
                    usage,
                }
            }
        };
        let tools = serde_json::to_value(&generated.tool_use).unwrap_or_else(|_| json!([]));
        let draft = spec(
            EffectKind::DraftSummary,
            json!({
                "v": 1,
                "operation": operation.to_string(),
                "step": turn.to_string(),
                "tools": tools,
            }),
            after(shared, jiff::SignedDuration::ZERO),
            DRAFT_ATTEMPTS,
            Some(format!("draft:{turn}")),
        );
        debug_assert!(latest_turn(&snapshot, None).is_some());
        conclude(shared, held, operation, Some(ended), vec![draft]).await
    }
}
