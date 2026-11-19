//! The operation trace: the frozen, read-only view a UI or a report is built from.
//!
//! The schema is versioned. Adding step kinds, outcomes, edge kinds or optional fields is not
//! a breaking change; renaming, removing or changing the meaning of anything bumps
//! [`TRACE_SCHEMA_VERSION`]. The trace never carries step output, intent, approval details or
//! plan hashes.

use std::collections::HashMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::id::{ExecutionId, IncidentId, OperationId, StepId};

use super::aggregate::PinnedSubject;
use super::budget::Budgets;
use super::edge::EdgeKind;
use super::snapshot::OperationSnapshot;
use super::status::{OperationKind, PauseReason, WaitKind};
use super::step::{MAX_SUMMARY_CHARS, StepKind, StepOutcome, cut_chars};

/// The version of the trace schema.
pub const TRACE_SCHEMA_VERSION: u32 = 1;

/// The trace of one operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationTrace {
    /// [`TRACE_SCHEMA_VERSION`] at the time of writing.
    pub schema_version: u32,
    /// The journal sequence number the trace is as of.
    pub as_of_seq: u64,
    /// The operation.
    pub operation: TraceOperation,
    /// The steps in ordinal order.
    pub steps: Vec<TraceStep>,
    /// The edges, sorted by ordinal of `from`, then of `to`, then kind name.
    pub edges: Vec<TraceEdge>,
}

/// The operation part of a trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceOperation {
    /// The operation's id.
    pub id: OperationId,
    /// What it is for.
    pub kind: OperationKind,
    /// Its goal.
    pub goal: String,
    /// The status name, for example `waiting_approval`.
    pub status: String,
    /// Why it is paused, when it is.
    pub pause_reason: Option<PauseReason>,
    /// What it is waiting for, in priority order.
    pub waiting_for: Vec<WaitKind>,
    /// The current cycle.
    pub cycle: u32,
    /// Limits and use.
    pub budgets: Budgets,
    /// What it may act on.
    pub pinned: Vec<PinnedSubject>,
    /// The operation it was started from.
    pub parent: Option<OperationId>,
    /// The incidents it is about.
    pub incidents: Vec<IncidentId>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it started.
    pub started_at: Option<Timestamp>,
    /// When it ended.
    pub ended_at: Option<Timestamp>,
}

/// One step in a trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceStep {
    /// The step's id.
    pub id: StepId,
    /// The step it follows from.
    pub parent: Option<StepId>,
    /// What it is.
    pub kind: StepKind,
    /// The phase label.
    pub phase: String,
    /// The cycle it belongs to.
    pub cycle: u32,
    /// How far it got.
    pub outcome: StepOutcome,
    /// When it started.
    pub started_at: Option<Timestamp>,
    /// When it ended.
    pub ended_at: Option<Timestamp>,
    /// A one-line summary, at most 280 characters.
    pub summary: Option<String>,
    /// Things this step links to.
    pub refs: Vec<TraceRef>,
}

/// A link from a step to something else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceRef {
    /// What is linked, for example `execution`.
    pub kind: String,
    /// The linked thing's id.
    pub id: String,
}

/// One edge in a trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEdge {
    /// The dependent, later step.
    pub from: StepId,
    /// The prerequisite, earlier step.
    pub to: StepId,
    /// What the edge says.
    pub kind: EdgeKind,
}

/// Builds the trace of `snapshot` as of journal sequence `as_of_seq`. Pure.
#[must_use]
pub fn build_trace(snapshot: &OperationSnapshot, as_of_seq: u64) -> OperationTrace {
    let op = &snapshot.operation;
    let mut ordered: Vec<_> = snapshot.steps.iter().collect();
    ordered.sort_by_key(|record| record.step.ordinal);
    let ordinals: HashMap<StepId, u32> = ordered
        .iter()
        .map(|record| (record.step.id, record.step.ordinal))
        .collect();
    let ordinal_of = |id: StepId| ordinals.get(&id).copied().unwrap_or(u32::MAX);

    let steps = ordered
        .iter()
        .map(|record| TraceStep {
            id: record.step.id,
            parent: record.step.parent,
            kind: record.step.kind,
            phase: record.step.phase.clone(),
            cycle: record.step.cycle,
            outcome: record.progress.outcome,
            started_at: record.progress.started_at,
            ended_at: record.progress.ended_at,
            summary: record
                .progress
                .summary
                .as_deref()
                .map(|s| cut_chars(s, MAX_SUMMARY_CHARS)),
            refs: if record.step.kind == StepKind::Execution {
                vec![TraceRef {
                    kind: "execution".to_owned(),
                    id: ExecutionId::from_step(record.step.id).to_string(),
                }]
            } else {
                Vec::new()
            },
        })
        .collect();

    let mut edges: Vec<_> = snapshot.edges.iter().collect();
    edges.sort_by_key(|e| (ordinal_of(e.from), ordinal_of(e.to), e.kind.as_str()));

    OperationTrace {
        schema_version: TRACE_SCHEMA_VERSION,
        as_of_seq,
        operation: TraceOperation {
            id: op.id,
            kind: op.kind,
            goal: op.goal.clone(),
            status: op.status.name().to_owned(),
            pause_reason: op.status.pause_reason().cloned(),
            waiting_for: snapshot.waits(),
            cycle: op.cycle,
            budgets: op.budgets.clone(),
            pinned: op.pinned.clone(),
            parent: op.parent,
            incidents: snapshot.incidents.clone(),
            created_at: op.created_at,
            started_at: op.started_at,
            ended_at: op.ended_at,
        },
        steps,
        edges: edges
            .into_iter()
            .map(|e| TraceEdge {
                from: e.from,
                to: e.to,
                kind: e.kind,
            })
            .collect(),
    }
}
