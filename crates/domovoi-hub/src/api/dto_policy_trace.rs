//! Wire shapes of recorded shadow decisions, the simulator and the policy overview.

use std::collections::BTreeMap;

use domovoi_core::policy_store::{ShadowRecord, ShadowState, ShadowSummary};
use domovoi_engine::policy_shadow::{
    Comparison, DecisionEnvelope, Replayed, SimulationSummary, decode_inputs,
};
use serde::Serialize;
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::dto_policy::{DraftSummaryDto, VersionSummaryDto};
use crate::services::{SimulationItem, SimulationResult};

/// What the evaluator decided, or why it refused, as the JSON recorded decisions and simulated
/// ones share. A decision carries `outcome`, `reason`, the rules that matched and won, the
/// constraints, the hard limits applied, the facts used and the warnings; a refusal is
/// `{ "error": "<code>" }`.
fn decision_json(envelope: &DecisionEnvelope) -> Value {
    match envelope {
        DecisionEnvelope::Decision(doc) => serde_json::to_value(&**doc).unwrap_or(Value::Null),
        DecisionEnvelope::Error { code } => json!({ "error": code }),
    }
}

/// How a rule set's counts changed across a batch of re-run decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SimulationSummaryDto {
    /// Rows looked at.
    pub evaluated: u32,
    /// Rows whose decision did not change.
    pub same: u32,
    /// Rows whose decision changed.
    pub changed: u32,
    /// Rows that could not be replayed.
    pub not_replayable: u32,
    /// Changed rows that are now `allow`.
    pub newly_allowed: u32,
    /// Changed rows that are now `deny`.
    pub newly_denied: u32,
    /// Changed rows that now `require_approval`.
    pub newly_requires_approval: u32,
}

impl From<&SimulationSummary> for SimulationSummaryDto {
    fn from(summary: &SimulationSummary) -> Self {
        Self {
            evaluated: summary.evaluated,
            same: summary.same,
            changed: summary.changed,
            not_replayable: summary.not_replayable,
            newly_allowed: summary.newly_allowed,
            newly_denied: summary.newly_denied,
            newly_requires_approval: summary.newly_requires_approval,
        }
    }
}

/// One re-run decision next to the recorded one.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ReplayedDto {
    /// `same`, `changed` or `not_replayable`.
    pub comparison: String,
    /// What the row recorded, when it could be read: a decision, or `{ "error": "<code>" }`.
    #[schema(value_type = Option<Object>)]
    pub baseline: Option<Value>,
    /// What the rules decide now: a decision, or `{ "error": "<code>" }`.
    #[schema(value_type = Object)]
    pub simulated: Value,
}

impl From<&Replayed> for ReplayedDto {
    fn from(replayed: &Replayed) -> Self {
        Self {
            comparison: match replayed.comparison {
                Comparison::Same => "same",
                Comparison::Changed => "changed",
                Comparison::NotReplayable => "not_replayable",
            }
            .to_owned(),
            baseline: replayed.baseline.as_ref().map(decision_json),
            simulated: decision_json(&replayed.simulated),
        }
    }
}

/// One recorded step with its re-run decisions.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SimulationItemDto {
    /// The Execution step.
    pub execution_id: String,
    /// Its operation.
    pub operation_id: String,
    /// Its host.
    pub host_id: String,
    /// Its contract.
    pub contract: String,
    /// When the decision was recorded.
    pub evaluated_at: String,
    /// The policy version it was recorded under; 0 when none was published.
    pub recorded_policy_version: u32,
    /// Whether some input was unavailable when it was recorded.
    pub degraded: bool,
    /// The inputs that were unavailable and why.
    #[schema(value_type = Vec<Object>)]
    pub gaps: Value,
    /// The enforced-rules decision, when asked for.
    pub enforced: Option<ReplayedDto>,
    /// The shadow-rules decision, when asked for.
    pub shadow: Option<ReplayedDto>,
}

impl From<&SimulationItem> for SimulationItemDto {
    fn from(item: &SimulationItem) -> Self {
        let row = &item.record;
        Self {
            execution_id: row.execution.to_string(),
            operation_id: row.operation.to_string(),
            host_id: row.host.to_string(),
            contract: row.contract.clone(),
            evaluated_at: row.evaluated_at.to_string(),
            recorded_policy_version: row.policy_version,
            degraded: row.degraded,
            gaps: serde_json::from_str(&row.gaps).unwrap_or_else(|_| json!([])),
            enforced: item.enforced.as_ref().map(ReplayedDto::from),
            shadow: item.shadow.as_ref().map(ReplayedDto::from),
        }
    }
}

/// What a simulation found.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SimulationDto {
    /// Which rules were simulated, in words.
    pub label: String,
    /// Totals over every re-run.
    pub summary: SimulationSummaryDto,
    /// Whether more rows matched than were taken.
    pub truncated: bool,
    /// Standing notes.
    pub notes: Vec<String>,
    /// The rows.
    pub items: Vec<SimulationItemDto>,
}

impl From<&SimulationResult> for SimulationDto {
    fn from(result: &SimulationResult) -> Self {
        Self {
            label: result.label.clone(),
            summary: SimulationSummaryDto::from(&result.summary),
            truncated: result.truncated,
            notes: result.notes.iter().map(|n| (*n).to_owned()).collect(),
            items: result.items.iter().map(SimulationItemDto::from).collect(),
        }
    }
}

/// One recorded decision in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ShadowSummaryRowDto {
    /// The Execution step.
    pub execution_id: String,
    /// Its operation.
    pub operation_id: String,
    /// Its host.
    pub host_id: String,
    /// Its contract.
    pub contract: String,
    /// When the decision was recorded.
    pub evaluated_at: String,
    /// `evaluated`, `unevaluable` or `skipped`.
    pub status: String,
    /// Why it is not evaluated, or `late`.
    pub reason: Option<String>,
    /// The policy version it was recorded under; 0 when none was published.
    pub policy_version: u32,
    /// What the enforced rules decided.
    pub enforced_outcome: Option<String>,
    /// What the shadow rules decided.
    pub shadow_outcome: Option<String>,
    /// Whether some input was unavailable.
    pub degraded: bool,
}

impl From<&ShadowRecord> for ShadowSummaryRowDto {
    fn from(row: &ShadowRecord) -> Self {
        Self {
            execution_id: row.execution.to_string(),
            operation_id: row.operation.to_string(),
            host_id: row.host.to_string(),
            contract: row.contract.clone(),
            evaluated_at: row.evaluated_at.to_string(),
            status: row.status.as_str().to_owned(),
            reason: row.reason.clone(),
            policy_version: row.policy_version,
            enforced_outcome: row.enforced_outcome.clone(),
            shadow_outcome: row.shadow_outcome.clone(),
            degraded: row.degraded,
        }
    }
}

/// A page of recorded decisions, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ShadowPageDto {
    /// The rows.
    pub items: Vec<ShadowSummaryRowDto>,
    /// Pass as `cursor` for the next page; absent on the last.
    pub next_cursor: Option<String>,
}

/// One recorded decision with everything needed to replay it.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ShadowDetailDto {
    /// The list shape.
    #[serde(flatten)]
    pub row: ShadowSummaryRowDto,
    /// The inputs that were unavailable and why.
    #[schema(value_type = Vec<Object>)]
    pub gaps: Value,
    /// The step that was evaluated.
    #[schema(value_type = Object)]
    pub candidate: Value,
    /// The exact inputs the evaluator was given; none unless the row is `evaluated`.
    #[schema(value_type = Option<Object>)]
    pub inputs: Option<Value>,
    /// The enforced-rules decision: a decision, or `{ "error": "<code>" }`.
    #[schema(value_type = Option<Object>)]
    pub enforced: Option<Value>,
    /// The shadow-rules decision.
    #[schema(value_type = Option<Object>)]
    pub shadow: Option<Value>,
}

impl From<&ShadowRecord> for ShadowDetailDto {
    fn from(row: &ShadowRecord) -> Self {
        let decision = |text: &Option<String>| {
            text.as_deref()
                .and_then(DecisionEnvelope::from_json)
                .map(|envelope| decision_json(&envelope))
        };
        Self {
            row: ShadowSummaryRowDto::from(row),
            gaps: serde_json::from_str(&row.gaps).unwrap_or_else(|_| json!([])),
            candidate: serde_json::from_str(&row.candidate).unwrap_or(Value::Null),
            inputs: row
                .inputs
                .as_deref()
                .and_then(|text| decode_inputs(text).ok())
                .and_then(|inputs| serde_json::to_value(inputs).ok()),
            enforced: decision(&row.enforced),
            shadow: decision(&row.shadow),
        }
    }
}

/// What the recorder did in the last 24 hours.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Last24hDto {
    /// Decisions recorded.
    pub total: u64,
    /// By status.
    pub by_status: BTreeMap<String, u64>,
    /// By enforced outcome.
    pub by_enforced_outcome: BTreeMap<String, u64>,
    /// Decisions with some input unavailable.
    pub degraded: u64,
    /// Decisions whose enforced and shadow outcomes differ.
    pub differing: u64,
}

/// The shadow recorder's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ShadowStateDto {
    /// When the last sweep committed.
    pub last_sweep_at: Option<String>,
    /// Why the last sweep failed, if it did.
    pub last_error: Option<String>,
    /// Decisions evaluated, ever.
    pub total_evaluated: u64,
    /// Steps whose inputs could not be established, ever.
    pub total_unevaluable: u64,
    /// Steps never evaluated, ever.
    pub total_skipped: u64,
    /// The last 24 hours.
    pub last_24h: Last24hDto,
}

impl ShadowStateDto {
    /// The state with the summary of the last 24 hours.
    #[must_use]
    pub fn new(state: &ShadowState, summary: &ShadowSummary) -> Self {
        Self {
            last_sweep_at: state.last_sweep_at.map(|at| at.to_string()),
            last_error: state.last_error.clone(),
            total_evaluated: state.total_evaluated,
            total_unevaluable: state.total_unevaluable,
            total_skipped: state.total_skipped,
            last_24h: Last24hDto {
                total: summary.total,
                by_status: summary.by_status.clone(),
                by_enforced_outcome: summary.by_enforced_outcome.clone(),
                degraded: summary.degraded,
                differing: summary.differing,
            },
        }
    }
}

/// The policy at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PolicyOverviewDto {
    /// Whether `[policy] shadow` is on.
    pub shadow_enabled: bool,
    /// The zone `time_window` conditions are read in; the UI must label it.
    pub time_window_zone: String,
    /// The active version, if any was published.
    pub active: Option<VersionSummaryDto>,
    /// The draft, if there is one.
    pub draft: Option<DraftSummaryDto>,
    /// The shadow recorder.
    pub shadow: ShadowStateDto,
    /// Standing notes.
    pub notes: Vec<String>,
}
