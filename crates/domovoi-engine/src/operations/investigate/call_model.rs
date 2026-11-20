//! `CallModel`: the provider chain, the tool loop and the report parse, in one reasoning turn.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::meta::ToolSet;
use domovoi_core::ports::{ClaimToken, EffectKind, EffectRecord};
use domovoi_core::{
    FinishOutcome, HostId, OperationSnapshot, StepId, StepKind, StepOutcome, StepUsage,
};
use serde_json::json;

use super::DRAFT_ATTEMPTS;
use super::plumbing::{claim, end_operation, finish_turn, incident_of, runtime, short, step_of};
use super::turn_output::{TurnKind, TurnOutput};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::decide::RecordedStep;
use crate::operations::effects::{Address, address, after, finish, load, spec, unreadable};
use crate::operations::writer::{Conclusion, OpShared};
use crate::plan::PlanScope;
use crate::triage::agent::RunUsage;
use crate::triage::meta::{EvidenceRecord, ExperienceAccess, LoopScope};
use crate::triage::run::{GeneratedEnd, failed, generate, succeeded};
use crate::triage::tools::EvidenceClass;

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

/// What the model may do in a turn of the operation in `snapshot`: the investigation tools
/// (`ask_user` only when `questions` allows it), the operation's pinned scope, its evidence
/// budget and the questions it already asked.
fn loop_scope(snapshot: &OperationSnapshot, host: HostId, questions: bool) -> LoopScope {
    let budgets = &snapshot.operation.budgets;
    let asked = snapshot
        .steps
        .iter()
        .filter(|record| record.step.kind == StepKind::Question)
        .count();
    LoopScope {
        tools: ToolSet::Investigate { questions },
        plan: PlanScope::for_operation(snapshot),
        evidence_left: budgets
            .limits
            .evidence_requests
            .map(|limit| limit.saturating_sub(budgets.used.evidence_requests)),
        questions_asked: u32::try_from(asked).unwrap_or(u32::MAX),
        experience: ExperienceAccess::host(host, Some(snapshot.operation.id)),
    }
}

/// The reads of a turn as the evidence steps the writer stores, with fresh ids.
fn recorded(host: HostId, evidence: &[EvidenceRecord]) -> Vec<RecordedStep> {
    evidence
        .iter()
        .filter_map(|record| {
            let mut args = serde_json::to_value(&record.call).ok()?;
            let tool = args.as_object_mut()?.remove("tool")?.as_str()?.to_owned();
            let outcome = match record.class {
                EvidenceClass::Observed => StepOutcome::Succeeded,
                EvidenceClass::Unavailable(_) => StepOutcome::Unavailable,
                EvidenceClass::Failed(_) => StepOutcome::Failed,
                EvidenceClass::Refused(_) => StepOutcome::Refused,
            };
            Some(RecordedStep {
                id: StepId::new(),
                host,
                tool,
                args,
                outcome,
                summary: Some(record.summary.clone()),
                reason: record.class.reason().map(short),
                output: Some(record.output.clone()),
            })
        })
        .collect()
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
        let scope = loop_scope(&snapshot, incident.host(), runtime.settings.allow_questions);
        let generated = generate(triage, &incident, &skills, bundle, scope).await;
        let usage = step_usage(generated.usage);
        // The reads are recorded whether the turn succeeds or fails.
        let evidence = recorded(incident.host(), &generated.evidence);
        let ended = match generated.result {
            Ok(GeneratedEnd::Report(report)) => {
                succeeded(triage).await;
                let summary = short(&report.headline);
                let mut output = TurnOutput::of(TurnKind::Report);
                output.report = Some(report);
                output.plan = generated.plan;
                output.dropped = generated.dropped;
                Conclusion {
                    step: turn,
                    outcome: StepOutcome::Succeeded,
                    summary: Some(summary),
                    reason: None,
                    output: Some(output.to_json()),
                    usage,
                }
            }
            Ok(GeneratedEnd::Asked(question)) => {
                succeeded(triage).await;
                let summary = short(&question);
                let mut output = TurnOutput::of(TurnKind::Asked);
                output.question = Some(question);
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
        finish_turn(shared, held, operation, ended, evidence, vec![draft]).await
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::{ReadTool, ReadToolCall, StepIntent};
    use serde_json::{Value, json};

    use super::*;

    /// Arguments for every read tool, with the optional ones both set and left out.
    fn samples() -> Vec<(&'static str, Value)> {
        vec![
            ("docker_ps", json!({})),
            ("docker_ps", json!({"all": true})),
            ("docker_logs", json!({"container": "nginx"})),
            ("docker_logs", json!({"container": "nginx", "lines": 50})),
            ("docker_inspect", json!({"container": "nginx"})),
            ("docker_events", json!({})),
            (
                "docker_events",
                json!({"container": "nginx", "since_minutes": 30}),
            ),
            ("host_overview", json!({})),
            ("journal", json!({})),
            (
                "journal",
                json!({"unit": "ssh", "since_minutes": 10, "lines": 20, "kernel": true}),
            ),
            ("listening_ports", json!({})),
            ("smart_report", json!({"device": "/dev/sda"})),
            ("incident_history", json!({})),
            ("incident_history", json!({"limit": 3})),
            ("notes_search", json!({"query": "disk"})),
            ("notes_search", json!({"query": "disk", "limit": 2})),
            ("run_read_skill", json!({"skill": "disk_report"})),
            (
                "run_read_skill",
                json!({"skill": "disk_report", "args": {"path": "/"}}),
            ),
        ]
    }

    /// The args a recorded read stores must parse back to the very same call, or the writer
    /// would drop every read of the turn.
    #[test]
    fn recorded_reads_round_trip_for_every_read_tool() {
        let mut seen: Vec<&str> = Vec::new();
        for (tool, args) in samples() {
            let call = ReadToolCall::from_json(tool, args.clone())
                .unwrap_or_else(|err| panic!("{tool} {args}: {err}"));
            let record = EvidenceRecord {
                call: call.clone(),
                class: EvidenceClass::Observed,
                summary: "seen".to_owned(),
                output: "text".to_owned(),
            };
            let steps = recorded(HostId::new(), &[record]);
            assert_eq!(steps.len(), 1, "{tool} {args}");
            let step = &steps[0];
            assert_eq!(step.tool, tool);
            let intent = StepIntent::EvidenceRequest {
                host: step.host,
                tool: step.tool.clone(),
                args: step.args.clone(),
            };
            intent
                .validate()
                .unwrap_or_else(|err| panic!("{tool} {args}: {err}"));
            let again = ReadToolCall::from_json(&step.tool, step.args.clone()).expect("parses");
            assert_eq!(again, call, "{tool} {args}");
            seen.push(tool);
        }
        for tool in ReadTool::ALL {
            assert!(
                seen.contains(&tool.name()),
                "{} is not covered",
                tool.name()
            );
        }
    }
}
