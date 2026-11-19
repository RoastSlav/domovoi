//! The trace DTOs: the read-only view the timeline and the graph are drawn from, field for
//! field the frozen trace schema. Core types are mapped here and never exposed directly.

use domovoi_core::{BudgetLimits, BudgetUsed, Budgets, OperationTrace, PauseReason, PinnedSubject};
use serde::Serialize;
use utoipa::ToSchema;

/// Why an operation is paused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PauseReasonDto {
    /// `budget`, `outcome_unknown`, `global_pause` or `error`.
    #[serde(rename = "type")]
    pub kind: String,
    /// For `budget`: the limit that ran out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exhausted: Option<String>,
    /// For `outcome_unknown`: the Execution step whose result is not known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// For `error`: what went wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl From<&PauseReason> for PauseReasonDto {
    fn from(reason: &PauseReason) -> Self {
        // The core type serialises as `{"type": .., ..}`; reading the fields from there keeps
        // the two in step without a second list of names.
        let value = serde_json::to_value(reason).unwrap_or_default();
        let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        Self {
            kind: text("type").unwrap_or_default(),
            exhausted: text("exhausted"),
            step: text("step"),
            message: text("message"),
        }
    }
}

/// The limits of an operation; `null` means no limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BudgetLimitsDto {
    /// Model turns.
    pub model_turns: Option<u32>,
    /// Evidence requests.
    pub evidence_requests: Option<u32>,
    /// Executions.
    pub executions: Option<u32>,
    /// Investigation cycles.
    pub cycles: Option<u32>,
    /// Wall-clock seconds.
    pub wall_seconds: Option<u64>,
    /// Model tokens.
    pub tokens: Option<u64>,
    /// Model cost in millionths of a US dollar.
    pub cost_micros: Option<u64>,
}

impl From<&BudgetLimits> for BudgetLimitsDto {
    fn from(l: &BudgetLimits) -> Self {
        Self {
            model_turns: l.model_turns,
            evidence_requests: l.evidence_requests,
            executions: l.executions,
            cycles: l.cycles,
            wall_seconds: l.wall_seconds,
            tokens: l.tokens,
            cost_micros: l.cost_micros,
        }
    }
}

/// What an operation has used so far.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BudgetUsedDto {
    /// Model turns.
    pub model_turns: u32,
    /// Evidence requests.
    pub evidence_requests: u32,
    /// Executions.
    pub executions: u32,
    /// Wall-clock seconds.
    pub wall_seconds: u64,
    /// Model tokens.
    pub tokens: u64,
    /// Model cost in millionths of a US dollar.
    pub cost_micros: u64,
}

impl From<&BudgetUsed> for BudgetUsedDto {
    fn from(u: &BudgetUsed) -> Self {
        Self {
            model_turns: u.model_turns,
            evidence_requests: u.evidence_requests,
            executions: u.executions,
            wall_seconds: u.wall_seconds,
            tokens: u.tokens,
            cost_micros: u.cost_micros,
        }
    }
}

/// An operation's limits and use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BudgetsDto {
    /// The limits.
    pub limits: BudgetLimitsDto,
    /// The use.
    pub used: BudgetUsedDto,
}

impl From<&Budgets> for BudgetsDto {
    fn from(b: &Budgets) -> Self {
        Self {
            limits: BudgetLimitsDto::from(&b.limits),
            used: BudgetUsedDto::from(&b.used),
        }
    }
}

/// A host, check and target an operation may act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PinnedSubjectDto {
    /// Host id.
    pub host: String,
    /// The check, for example `container.down`.
    pub check: String,
    /// The target, for example `plex`.
    pub target: Option<String>,
}

impl From<&PinnedSubject> for PinnedSubjectDto {
    fn from(p: &PinnedSubject) -> Self {
        Self {
            host: p.host.to_string(),
            check: p.check.to_string(),
            target: p.target.as_ref().map(ToString::to_string),
        }
    }
}

/// A link from a step to something stored elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TraceRefDto {
    /// What it points at, for example `execution`.
    pub kind: String,
    /// Its id.
    pub id: String,
}

/// A link between two steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TraceEdgeDto {
    /// The dependent step.
    pub from: String,
    /// The earlier step it points at.
    pub to: String,
    /// `depends_on`, `caused_by`, `verifies`, `retry_of` or `answers`.
    pub kind: String,
}

/// One step as the timeline shows it. Never carries output, intent or approval details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TraceStepDto {
    /// Step id.
    pub id: String,
    /// The step it follows from.
    pub parent: Option<String>,
    /// The step kind, for example `reasoning_turn`.
    pub kind: String,
    /// A short label of the phase of work.
    pub phase: String,
    /// The cycle it belongs to.
    pub cycle: u32,
    /// The outcome so far, for example `running` or `succeeded`.
    pub outcome: String,
    /// When it started.
    pub started_at: Option<String>,
    /// When it ended.
    pub ended_at: Option<String>,
    /// One line on what it came to.
    pub summary: Option<String>,
    /// What it links to.
    pub refs: Vec<TraceRefDto>,
}

/// The operation a trace is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TraceOperationDto {
    /// Operation id.
    pub id: String,
    /// The kind, for example `investigate`.
    pub kind: String,
    /// What it is trying to achieve.
    pub goal: String,
    /// The status name, for example `waiting_approval`.
    pub status: String,
    /// Why it is paused, when it is.
    pub pause_reason: Option<PauseReasonDto>,
    /// What it waits for: `approval`, `user`, `agent` or `timer`.
    pub waiting_for: Vec<String>,
    /// The current cycle.
    pub cycle: u32,
    /// Limits and use.
    pub budgets: BudgetsDto,
    /// What it may act on.
    pub pinned: Vec<PinnedSubjectDto>,
    /// The operation it was started from.
    pub parent: Option<String>,
    /// The incidents it is about.
    pub incidents: Vec<String>,
    /// When it was created.
    pub created_at: String,
    /// When it started.
    pub started_at: Option<String>,
    /// When it ended.
    pub ended_at: Option<String>,
}

/// The read-only view the timeline and the graph are drawn from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OperationTraceDto {
    /// The version of this layout.
    pub schema_version: u32,
    /// The journal position it was read at; apply only events with a greater `seq`.
    pub as_of_seq: u64,
    /// The operation.
    pub operation: TraceOperationDto,
    /// Its steps in order.
    pub steps: Vec<TraceStepDto>,
    /// The links between them.
    pub edges: Vec<TraceEdgeDto>,
}

pub(super) fn name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl From<&OperationTrace> for OperationTraceDto {
    fn from(t: &OperationTrace) -> Self {
        let o = &t.operation;
        Self {
            schema_version: t.schema_version,
            as_of_seq: t.as_of_seq,
            operation: TraceOperationDto {
                id: o.id.to_string(),
                kind: o.kind.as_str().to_owned(),
                goal: o.goal.clone(),
                status: o.status.clone(),
                pause_reason: o.pause_reason.as_ref().map(PauseReasonDto::from),
                waiting_for: o.waiting_for.iter().map(name).collect(),
                cycle: o.cycle,
                budgets: BudgetsDto::from(&o.budgets),
                pinned: o.pinned.iter().map(PinnedSubjectDto::from).collect(),
                parent: o.parent.map(|p| p.to_string()),
                incidents: o.incidents.iter().map(ToString::to_string).collect(),
                created_at: o.created_at.to_string(),
                started_at: o.started_at.map(|t| t.to_string()),
                ended_at: o.ended_at.map(|t| t.to_string()),
            },
            steps: t
                .steps
                .iter()
                .map(|s| TraceStepDto {
                    id: s.id.to_string(),
                    parent: s.parent.map(|p| p.to_string()),
                    kind: s.kind.as_str().to_owned(),
                    phase: s.phase.clone(),
                    cycle: s.cycle,
                    outcome: s.outcome.as_str().to_owned(),
                    started_at: s.started_at.map(|t| t.to_string()),
                    ended_at: s.ended_at.map(|t| t.to_string()),
                    summary: s.summary.clone(),
                    refs: s
                        .refs
                        .iter()
                        .map(|r| TraceRefDto {
                            kind: r.kind.clone(),
                            id: r.id.clone(),
                        })
                        .collect(),
                })
                .collect(),
            edges: t
                .edges
                .iter()
                .map(|e| TraceEdgeDto {
                    from: e.from.to_string(),
                    to: e.to.to_string(),
                    kind: e.kind.as_str().to_owned(),
                })
                .collect(),
        }
    }
}
