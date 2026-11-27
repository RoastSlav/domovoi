//! What the API returns for operations: the list rows, the trace the screens render, the
//! executions a person approves and one step. Core types are mapped here and never exposed
//! directly. Ids are strings, times RFC 3339 with a trailing `Z`, and every enumeration is a
//! string a consumer shows generically when it does not know it.

use std::collections::BTreeMap;

use domovoi_core::{ExecutionRecord, Operation, OperationSnapshot, StepRecord};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::dto_experience::ResolutionBriefDto;
use super::dto_trace::name;
pub use super::dto_trace::{
    BudgetLimitsDto, BudgetUsedDto, BudgetsDto, OperationTraceDto, PauseReasonDto,
    PinnedSubjectDto, TraceEdgeDto, TraceOperationDto, TraceRefDto, TraceStepDto,
};

/// Filters of the operation list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OperationQuery {
    /// An operation status name, for example `running`.
    pub status: Option<String>,
    /// Only operations about this incident.
    pub incident: Option<String>,
    /// Only operations that act on this host (by name).
    pub host: Option<String>,
    /// An operation kind, for example `investigate`.
    pub kind: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 200. Default 50.
    pub limit: Option<u16>,
}

/// One operation in a list or on the incident page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OperationSummaryDto {
    /// Operation id.
    pub id: String,
    /// The kind, for example `investigate`.
    pub kind: String,
    /// What it is trying to achieve.
    pub goal: String,
    /// The status name, for example `running`.
    pub status: String,
    /// Why it is paused, when it is.
    pub pause_reason: Option<PauseReasonDto>,
    /// What it waits for: `approval`, `user`, `agent` or `timer`.
    pub waiting_for: Vec<String>,
    /// The current cycle.
    pub cycle: u32,
    /// The incidents it is about.
    pub incidents: Vec<String>,
    /// The hosts it may act on.
    pub hosts: Vec<String>,
    /// When it was created.
    pub created_at: String,
    /// When it started.
    pub started_at: Option<String>,
    /// When it ended.
    pub ended_at: Option<String>,
    /// When it last changed.
    pub updated_at: String,
    /// How a finished operation ended; `null` until its summary exists.
    pub resolution: Option<ResolutionBriefDto>,
    /// The question the operation waits on, only while it waits for a person's answer.
    pub question: Option<PendingQuestionDto>,
}

/// A question that triage asked the owner and still waits on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PendingQuestionDto {
    /// The question step; answer it with `POST /api/v1/operations/{id}/steps/{step}/answer`.
    pub step: String,
    /// The question as asked, redacted.
    pub text: String,
    /// When it stops waiting. After that the operation carries on without an answer.
    pub expires_at: String,
}

impl PendingQuestionDto {
    /// The unanswered question of `snapshot`, if there is one.
    #[must_use]
    pub fn of(snapshot: &OperationSnapshot) -> Option<Self> {
        snapshot.steps.iter().find_map(|record| {
            if record.step.kind != domovoi_core::StepKind::Question
                || record.progress.outcome.is_final()
            {
                return None;
            }
            let domovoi_core::StepIntent::Question { text, expires_at } = &record.step.intent
            else {
                return None;
            };
            Some(Self {
                step: record.step.id.to_string(),
                text: text.clone(),
                expires_at: expires_at.to_string(),
            })
        })
    }
}

impl OperationSummaryDto {
    /// The row for `operation`, with the incidents and waits only a snapshot knows.
    #[must_use]
    pub fn of(operation: &Operation, snapshot: Option<&OperationSnapshot>) -> Self {
        let mut hosts: Vec<String> = Vec::new();
        for subject in &operation.pinned {
            let host = subject.host.to_string();
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        Self {
            id: operation.id.to_string(),
            kind: operation.kind.as_str().to_owned(),
            goal: operation.goal.clone(),
            status: operation.status.name().to_owned(),
            pause_reason: operation.status.pause_reason().map(PauseReasonDto::from),
            waiting_for: snapshot
                .map(|s| s.waits().iter().map(name).collect())
                .unwrap_or_default(),
            cycle: operation.cycle,
            incidents: snapshot
                .map(|s| s.incidents.iter().map(ToString::to_string).collect())
                .unwrap_or_default(),
            hosts,
            created_at: operation.created_at.to_string(),
            started_at: operation.started_at.map(|t| t.to_string()),
            ended_at: operation.ended_at.map(|t| t.to_string()),
            updated_at: operation.updated_at.to_string(),
            resolution: None,
            question: snapshot.and_then(PendingQuestionDto::of),
        }
    }
}

/// One page of operations, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OperationPage {
    /// The operations.
    pub items: Vec<OperationSummaryDto>,
    /// Pass as `cursor` for the next page; absent on the last one.
    pub next_cursor: Option<String>,
    /// The journal position read before the page; apply only events with a greater `seq`.
    pub as_of_seq: u64,
}

/// A run of a capability that a person approves or denies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExecutionDto {
    /// The Execution step id, which is also the execution id.
    pub step: String,
    /// The Approval step that waits for a person.
    pub approval_step: Option<String>,
    /// The contract, for example `skill.restart_container`.
    pub contract: String,
    /// The contract version the plan was made against.
    pub contract_version: u32,
    /// The host it runs on.
    pub host_id: String,
    /// The check it is about.
    pub check: String,
    /// The target it is about.
    pub target: Option<String>,
    /// Arguments by parameter name.
    pub args: BTreeMap<String, String>,
    /// The derived state, for example `awaiting_approval` or `succeeded`.
    pub state: String,
    /// The plan hash a person approves, 64 hex characters.
    pub plan_hash: String,
    /// `read`, `write` or `destructive` from the host's current skills; `null` when the
    /// skill is not on the host now.
    pub risk: Option<String>,
    /// Whether approving needs the skill's name typed: for a destructive skill, and for one
    /// whose risk is not known.
    pub confirm_required: bool,
    /// When the approval window closes.
    pub expires_at: String,
    /// Who approved it, for example `user:<id>`.
    pub approved_by: Option<String>,
    /// When it was approved.
    pub approved_at: Option<String>,
    /// Why it was refused, not run, unavailable or failed.
    pub reason: Option<String>,
    /// Who authorized it, once somebody or a policy did.
    pub authorization: Option<AuthorizationDto>,
}

/// Who authorized an execution: a person, or a policy permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AuthorizationDto {
    /// `human` or `policy`.
    pub kind: String,
    /// The approving user, for a person: `user:<id>`.
    pub user: Option<String>,
    /// Where a person approved: `web`, or `chat:<platform>`.
    pub channel: Option<String>,
    /// The permit, for a policy.
    pub permit: Option<PolicyAllowDto>,
}

/// The policy decision that allowed an execution: shown to the owner, never accepted from a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PolicyAllowDto {
    /// The decision the permit packs.
    pub decision_id: String,
    /// The rules the decision applied.
    pub rules: Vec<String>,
    /// The policy version the decision used.
    pub policy_version: String,
}

impl AuthorizationDto {
    /// The authorization stored on `row`, if there is one.
    #[must_use]
    pub fn of(row: &ExecutionRecord) -> Option<Self> {
        if let Some(approval) = row.approval() {
            let channel = match approval.approval().channel() {
                domovoi_core::ApprovalChannel::Web { .. } => "web".to_owned(),
                domovoi_core::ApprovalChannel::Chat { platform, .. } => format!("chat:{platform}"),
            };
            return Some(Self {
                kind: "human".to_owned(),
                user: Some(format!("user:{}", approval.approval().user())),
                channel: Some(channel),
                permit: None,
            });
        }
        row.permit().map(|stored| Self {
            kind: "policy".to_owned(),
            user: None,
            channel: None,
            permit: Some(PolicyAllowDto {
                decision_id: stored.decision_id().to_owned(),
                rules: stored.permit().rule_ids().to_vec(),
                policy_version: stored.permit().policy_version().to_owned(),
            }),
        })
    }
}

impl ExecutionDto {
    /// The execution of Execution step `step`, with the risk the host's skills give it.
    #[must_use]
    pub fn of(
        snapshot: &OperationSnapshot,
        step: &StepRecord,
        row: &ExecutionRecord,
        risk: Option<String>,
    ) -> Self {
        let approval_step = snapshot.steps.iter().find(|candidate| {
            matches!(
                &candidate.step.intent,
                domovoi_core::StepIntent::Approval { execution, .. } if *execution == step.step.id
            )
        });
        let state = row.state(
            step.progress.outcome,
            approval_step.map(|record| record.progress.outcome),
        );
        let approval = row.approval();
        let confirm_required = !matches!(risk.as_deref(), Some("read" | "write"));
        Self {
            step: step.step.id.to_string(),
            approval_step: approval_step.map(|record| record.step.id.to_string()),
            contract: row.contract.to_string(),
            contract_version: row.contract_version,
            host_id: row.host.to_string(),
            check: row.check.to_string(),
            target: row.target.as_ref().map(ToString::to_string),
            args: row.args.clone(),
            state: state.name().to_owned(),
            plan_hash: row.plan_hash.to_string(),
            risk,
            confirm_required,
            expires_at: row.expires_at.to_string(),
            approved_by: approval.map(|a| format!("user:{}", a.approval().user())),
            approved_at: approval.map(|a| a.approval().at().to_string()),
            reason: step.progress.reason.clone(),
            authorization: AuthorizationDto::of(row),
        }
    }
}

/// One step with its stored intent and bounded output.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct StepDetailDto {
    /// Step id.
    pub id: String,
    /// The operation it belongs to.
    pub operation_id: String,
    /// Its position in the operation.
    pub ordinal: u32,
    /// The step kind.
    pub kind: String,
    /// A short label of the phase of work.
    pub phase: String,
    /// The step it follows from.
    pub parent: Option<String>,
    /// The cycle it belongs to.
    pub cycle: u32,
    /// The outcome so far.
    pub outcome: String,
    /// When it started.
    pub started_at: Option<String>,
    /// When it ended.
    pub ended_at: Option<String>,
    /// One line on what it came to.
    pub summary: Option<String>,
    /// Why it ended as it did.
    pub reason: Option<String>,
    /// What it produced, redacted and at most 16 KiB.
    pub output: Option<String>,
    /// What the step is for, as stored. Never changes.
    #[schema(value_type = Object)]
    pub intent: serde_json::Value,
    /// When it was created.
    pub created_at: String,
    /// A containment step only: its phase, source, TTL and how the helper's safety stood.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containment: Option<super::dto_containment::ContainmentStepDto>,
}

impl From<&StepRecord> for StepDetailDto {
    fn from(r: &StepRecord) -> Self {
        Self {
            id: r.step.id.to_string(),
            operation_id: r.step.operation.to_string(),
            ordinal: r.step.ordinal,
            kind: r.step.kind.as_str().to_owned(),
            phase: r.step.phase.clone(),
            parent: r.step.parent.map(|p| p.to_string()),
            cycle: r.step.cycle,
            outcome: r.progress.outcome.as_str().to_owned(),
            started_at: r.progress.started_at.map(|t| t.to_string()),
            ended_at: r.progress.ended_at.map(|t| t.to_string()),
            summary: r.progress.summary.clone(),
            reason: r.progress.reason.clone(),
            output: r.progress.output.clone(),
            intent: serde_json::to_value(&r.step.intent).unwrap_or_default(),
            created_at: r.step.created_at.to_string(),
            containment: None,
        }
    }
}

/// An operation, its trace and its executions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OperationDetailDto {
    /// The journal position read before loading; equals `trace.as_of_seq`.
    pub as_of_seq: u64,
    /// The operation.
    pub operation: OperationSummaryDto,
    /// The timeline and graph.
    pub trace: OperationTraceDto,
    /// The runs a person approves or denies.
    pub executions: Vec<ExecutionDto>,
}

/// Approving an execution step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ApproveStepRequest {
    /// Required. The v2 plan hash shown on the execution, 64 hex characters.
    #[schema(
        value_type = String,
        required = true,
        nullable = false,
        min_length = 64,
        max_length = 64,
        pattern = "^[0-9a-f]{64}$"
    )]
    pub plan_hash: Option<String>,
    /// Required when the execution says `confirm_required`: the skill's name typed exactly.
    pub confirm_text: Option<String>,
    /// A containment step only: the signed-in person's password, asked again.
    #[serde(default)]
    pub password: Option<String>,
    /// A containment step only: set when the person acknowledges that the helper's coverage
    /// could not be verified. Without it that step is refused with `unverified_not_acknowledged`.
    #[serde(default)]
    pub unverified_acknowledged: Option<bool>,
}

/// Cancelling an operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, ToSchema)]
pub struct CancelOperationRequest {
    /// Why, at most 500 characters. Empty or missing records "cancelled by a person".
    #[serde(default)]
    pub reason: Option<String>,
}

/// One limit to raise when resuming.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct BudgetExtensionDto {
    /// `model_turns`, `evidence_requests`, `executions`, `cycles`, `wall_time`, `tokens` or
    /// `cost`.
    pub kind: String,
    /// The new limit: seconds for `wall_time`, micro-units for `cost`, a count otherwise.
    /// 1 to 10000 for a count, 1 to 10000000000 otherwise.
    pub limit: u64,
}

/// Resuming a paused operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ResumeOperationRequest {
    /// Limits to raise first, at most 7, each kind once. A spent limit must be raised.
    #[serde(default)]
    pub extend: Vec<BudgetExtensionDto>,
}

/// Answering the question an operation waits on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct AnswerStepRequest {
    /// The answer, 1 to 2000 characters after trimming. It reaches the model as data.
    pub text: String,
}

/// The operation after a control, and the journal position it was read at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct OperationControlDto {
    /// The journal position read after the command and before the operation.
    pub as_of_seq: u64,
    /// The refreshed operation.
    pub operation: OperationSummaryDto,
}
