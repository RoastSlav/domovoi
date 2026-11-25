//! Rows of the operation tables and how they decode into domain types.
//!
//! Stored names (status, kind, outcome, edge kind) are validated here, in Rust, when read: an
//! unknown one is a backend error that names the value. A stored approval is loaded back with
//! `HumanApproval::restore`, the only place besides the unit of work's approval path that may
//! build one (INV-1, ADR 0014).

use domovoi_core::ports::{StoreError, Versioned};
use domovoi_core::{
    ApprovalChannel, ApprovalProvenance, ContractId, EdgeKind, ExecutionApproval, ExecutionParts,
    ExecutionRecord, HumanApproval, JournalEpoch, Operation, OperationKind, OperationStatus,
    PauseReason, PinnedSubject, PolicyPermit, PolicyPermitParts, Risk, Step, StepEdge, StepIntent,
    StepKind, StepOutcome, StepProgress, StepRecord, StoredPermit,
};

use crate::codec::{from_json, from_ms, from_ms_opt, from_name, parse, uint};
use crate::error::corrupt;

/// The columns of an operation row, in [`OperationRow`] order.
macro_rules! operation_columns {
    () => {
        "SELECT id, kind, goal, status, pause_reason, cycle, budgets, pinned, parent_id, \
         created_at, started_at, ended_at, updated_at, version FROM operations"
    };
}
pub(super) use operation_columns;

/// The columns of a step row, in [`StepRow`] order.
macro_rules! step_columns {
    () => {
        "SELECT id, operation_id, ordinal, kind, phase, parent_step_id, cycle, intent, \
         created_at, status, started_at, ended_at, summary, reason, output FROM steps"
    };
}
pub(super) use step_columns;

/// The columns of an execution row, in [`ExecutionRow`] order.
macro_rules! execution_columns {
    () => {
        "SELECT execution_id, operation_id, host_id, contract, contract_version, check_key, \
         target, args, plan_hash, expires_at, authorization_kind, approved_by, approved_channel, \
         approved_at, approved_plan_hash, approval_session_ref, approval_client, waiting_since, \
         agent_epoch, dispatched_at, settled_at, created_at, updated_at, version, risk, \n         chat_token_hash, \
         (SELECT decision_id FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_decision_id, \
         (SELECT policy_version FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_policy_version, \
         (SELECT ceiling_hash FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_ceiling_hash, \
         (SELECT plan_hash FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_plan_hash, \
         (SELECT issued_at FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_issued_at, \
         (SELECT expires_at FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_expires_at, \
         (SELECT decision FROM execution_permits p WHERE p.execution_id = executions.execution_id) AS permit_decision, \
         (SELECT group_concat(rule_id, ',') FROM execution_permit_rules r WHERE r.execution_id = executions.execution_id) AS permit_rules \
         FROM executions"
    };
}
pub(super) use execution_columns;

/// One row of `operations`.
#[derive(sqlx::FromRow)]
pub(super) struct OperationRow {
    id: String,
    kind: String,
    goal: String,
    status: String,
    pause_reason: Option<String>,
    cycle: i64,
    budgets: String,
    pinned: String,
    parent_id: Option<String>,
    created_at: i64,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    updated_at: i64,
    version: i64,
}

impl OperationRow {
    /// The operation and the version the row is at.
    pub(super) fn into_versioned(self) -> Result<Versioned<Operation>, StoreError> {
        let version = uint(self.version);
        Ok(Versioned {
            value: self.into_operation()?,
            version,
        })
    }

    /// The operation, validating the stored names.
    pub(super) fn into_operation(self) -> Result<Operation, StoreError> {
        let pause = self
            .pause_reason
            .as_deref()
            .map(|text| from_json::<PauseReason>(text, "operation pause reason"))
            .transpose()?;
        let status = OperationStatus::from_parts(&self.status, pause)
            .ok_or_else(|| corrupt("operation status", &self.status))?;
        let kind = OperationKind::parse(&self.kind)
            .ok_or_else(|| corrupt("operation kind", &self.kind))?;
        Ok(Operation {
            id: parse(&self.id, "operation id")?,
            kind,
            goal: self.goal,
            status,
            cycle: u32::try_from(self.cycle).map_err(|e| corrupt("operation cycle", e))?,
            budgets: from_json(&self.budgets, "operation budgets")?,
            pinned: from_json::<Vec<PinnedSubject>>(&self.pinned, "operation pinned subjects")?,
            parent: self
                .parent_id
                .as_deref()
                .map(|id| parse(id, "parent operation id"))
                .transpose()?,
            created_at: from_ms(self.created_at)?,
            started_at: from_ms_opt(self.started_at)?,
            ended_at: from_ms_opt(self.ended_at)?,
            updated_at: from_ms(self.updated_at)?,
        })
    }
}

/// One row of `steps`.
#[derive(sqlx::FromRow)]
pub(super) struct StepRow {
    id: String,
    operation_id: String,
    ordinal: i64,
    kind: String,
    phase: String,
    parent_step_id: Option<String>,
    cycle: i64,
    intent: String,
    created_at: i64,
    status: String,
    started_at: Option<i64>,
    ended_at: Option<i64>,
    summary: Option<String>,
    reason: Option<String>,
    output: Option<String>,
}

impl StepRow {
    /// The step with its progress, validating the stored names.
    pub(super) fn into_record(self) -> Result<StepRecord, StoreError> {
        let kind = StepKind::parse(&self.kind).ok_or_else(|| corrupt("step kind", &self.kind))?;
        let outcome =
            StepOutcome::parse(&self.status).ok_or_else(|| corrupt("step status", &self.status))?;
        let intent: StepIntent = from_json(&self.intent, "step intent")?;
        if intent.kind() != kind {
            return Err(corrupt("step intent", "does not match the step kind"));
        }
        Ok(StepRecord {
            step: Step {
                id: parse(&self.id, "step id")?,
                operation: parse(&self.operation_id, "operation id")?,
                ordinal: u32::try_from(self.ordinal).map_err(|e| corrupt("step ordinal", e))?,
                kind,
                phase: self.phase,
                parent: self
                    .parent_step_id
                    .as_deref()
                    .map(|id| parse(id, "parent step id"))
                    .transpose()?,
                cycle: u32::try_from(self.cycle).map_err(|e| corrupt("step cycle", e))?,
                intent,
                created_at: from_ms(self.created_at)?,
            },
            progress: StepProgress {
                outcome,
                started_at: from_ms_opt(self.started_at)?,
                ended_at: from_ms_opt(self.ended_at)?,
                summary: self.summary,
                reason: self.reason,
                output: self.output,
            },
        })
    }
}

/// One row of `step_edges`.
#[derive(sqlx::FromRow)]
pub(super) struct EdgeRow {
    pub(super) operation_id: String,
    pub(super) from_step: String,
    pub(super) to_step: String,
    pub(super) kind: String,
}

impl EdgeRow {
    /// The edge, validating the stored kind.
    pub(super) fn into_edge(self) -> Result<StepEdge, StoreError> {
        Ok(StepEdge {
            operation: parse(&self.operation_id, "operation id")?,
            from: parse(&self.from_step, "step id")?,
            to: parse(&self.to_step, "step id")?,
            kind: EdgeKind::parse(&self.kind).ok_or_else(|| corrupt("edge kind", &self.kind))?,
        })
    }
}

/// One row of `executions`.
#[derive(sqlx::FromRow)]
pub(super) struct ExecutionRow {
    execution_id: String,
    operation_id: String,
    host_id: String,
    contract: String,
    contract_version: i64,
    check_key: String,
    target: Option<String>,
    args: String,
    plan_hash: String,
    expires_at: i64,
    authorization_kind: Option<String>,
    approved_by: Option<String>,
    approved_channel: Option<String>,
    approved_at: Option<i64>,
    approved_plan_hash: Option<String>,
    approval_session_ref: Option<String>,
    approval_client: Option<String>,
    waiting_since: Option<i64>,
    agent_epoch: Option<String>,
    dispatched_at: Option<i64>,
    settled_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
    version: i64,
    risk: Option<String>,
    chat_token_hash: Option<Vec<u8>>,
    permit_decision_id: Option<String>,
    permit_policy_version: Option<String>,
    permit_ceiling_hash: Option<String>,
    permit_plan_hash: Option<String>,
    permit_issued_at: Option<i64>,
    permit_expires_at: Option<i64>,
    permit_decision: Option<String>,
    permit_rules: Option<String>,
}

impl ExecutionRow {
    /// The execution record and the version the row is at.
    pub(super) fn into_versioned(self) -> Result<Versioned<ExecutionRecord>, StoreError> {
        let version = uint(self.version);
        Ok(Versioned {
            value: self.into_record()?,
            version,
        })
    }

    /// The execution record, restoring the stored approval.
    pub(super) fn into_record(self) -> Result<ExecutionRecord, StoreError> {
        let approval = self.approval()?;
        let permit = self.permit()?;
        let dispatched = match (self.agent_epoch.as_deref(), self.dispatched_at) {
            (Some(epoch), Some(at)) => Some((
                from_name::<JournalEpoch>(epoch, "agent epoch")?,
                from_ms(at)?,
            )),
            (None, None) => None,
            _ => return Err(corrupt("execution dispatch", "partially stored")),
        };
        Ok(ExecutionRecord::restore(ExecutionParts {
            operation: parse(&self.operation_id, "operation id")?,
            step: parse(&self.execution_id, "step id")?,
            host: parse(&self.host_id, "host id")?,
            contract: from_name::<ContractId>(&self.contract, "contract id")?,
            contract_version: u32::try_from(self.contract_version)
                .map_err(|e| corrupt("contract version", e))?,
            check: from_name(&self.check_key, "check key")?,
            target: self
                .target
                .as_deref()
                .map(|target| from_name(target, "target key"))
                .transpose()?,
            args: from_json(&self.args, "execution args")?,
            plan_hash: from_name(&self.plan_hash, "plan hash")?,
            expires_at: from_ms(self.expires_at)?,
            risk: self
                .risk
                .as_deref()
                .map(|risk| Risk::from_name(risk).ok_or_else(|| corrupt("execution risk", risk)))
                .transpose()?,
            chat_token_hash: self
                .chat_token_hash
                .as_deref()
                .map(|hash| {
                    <[u8; 32]>::try_from(hash)
                        .map_err(|_| corrupt("chat token hash", "wrong length"))
                })
                .transpose()?,
            approval,
            permit,
            waiting_since: from_ms_opt(self.waiting_since)?,
            dispatched,
            settled_at: from_ms_opt(self.settled_at)?,
            created_at: from_ms(self.created_at)?,
            updated_at: from_ms(self.updated_at)?,
        }))
    }

    /// The stored approval: all of its columns or none. Only a human approval is ever stored.
    fn approval(&self) -> Result<Option<ExecutionApproval>, StoreError> {
        let partial = || corrupt("execution approval", "partially stored");
        match (
            self.authorization_kind.as_deref(),
            self.approved_by.as_deref(),
            self.approved_channel.as_deref(),
            self.approved_at,
            self.approved_plan_hash.as_deref(),
            self.approval_client.as_deref(),
        ) {
            (None, None, None, None, None, None) if self.approval_session_ref.is_none() => Ok(None),
            (Some("human"), Some(user), Some(channel), Some(at), Some(displayed), Some(client)) => {
                let human = HumanApproval::restore(
                    parse(user, "approver")?,
                    from_json::<ApprovalChannel>(channel, "approval channel")?,
                    from_ms(at)?,
                );
                Ok(Some(ExecutionApproval::restore(
                    human,
                    from_name(displayed, "approved plan hash")?,
                    ApprovalProvenance::new(self.approval_session_ref.clone(), client),
                )))
            }
            // A policy authorization stores its permit in `execution_permits`, not here.
            (Some("policy"), None, None, None, None, None)
                if self.approval_session_ref.is_none() =>
            {
                Ok(None)
            }
            (Some(other), ..) if other != "human" => Err(corrupt("authorization kind", other)),
            _ => Err(partial()),
        }
    }
}

impl ExecutionRow {
    /// The stored permit: all of its columns or none. Restored with `PolicyPermit::restore`,
    /// the only place besides tests that may call it.
    fn permit(&self) -> Result<Option<StoredPermit>, StoreError> {
        let partial = || corrupt("execution permit", "partially stored");
        let Some(decision_id) = self.permit_decision_id.as_deref() else {
            if self.authorization_kind.as_deref() == Some("policy") {
                return Err(corrupt(
                    "execution permit",
                    "authorized by policy without a permit",
                ));
            }
            return Ok(None);
        };
        let (Some(version), Some(ceiling), Some(plan), Some(issued), Some(expires), Some(decision)) = (
            self.permit_policy_version.as_deref(),
            self.permit_ceiling_hash.as_deref(),
            self.permit_plan_hash.as_deref(),
            self.permit_issued_at,
            self.permit_expires_at,
            self.permit_decision.clone(),
        ) else {
            return Err(partial());
        };
        let rules: Vec<String> = self
            .permit_rules
            .as_deref()
            .map(|text| text.split(',').map(str::to_owned).collect())
            .unwrap_or_default();
        let permit = PolicyPermit::restore(PolicyPermitParts {
            decision_id,
            rule_ids: rules,
            policy_version: version,
            plan_hash: from_name(plan, "permit plan hash")?,
            issued_at: from_ms(issued)?,
            expires_at: from_ms(expires)?,
            ceiling_hash: ceiling,
            safety_identity: None,
        })
        .map_err(|e| corrupt("execution permit", e))?;
        Ok(Some(StoredPermit::new(permit, decision)))
    }
}
