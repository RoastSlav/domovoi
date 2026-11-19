//! Rows of the policy tables and how they decode into domain types. Stored names are
//! validated here: an unknown one is a backend error that names the value.

use std::collections::BTreeMap;

use domovoi_core::policy_store::{
    AckRecord, PolicyStatus, PolicyVersionRecord, ShadowCandidate, ShadowRecord, ShadowState,
    ShadowStatus, StoredRule,
};
use domovoi_core::ports::StoreError;
use domovoi_core::{Actor, ContractId, PinnedSubject};

use crate::codec::{from_json, from_ms, from_ms_opt, from_name, parse, uint};
use crate::error::corrupt;

/// The columns of a version row, in [`VersionRow`] order.
macro_rules! version_columns {
    () => {
        "SELECT id, status, number, draft_revision, based_on, rules_hash, rule_count, \
         locked_count, shadow_count, created_at, created_by, updated_at, updated_by, \
         published_at, published_by FROM policy_versions"
    };
}
pub(crate) use version_columns;

/// The columns of a decision row, in [`ShadowRow`] order.
macro_rules! shadow_columns {
    () => {
        "SELECT execution_id, operation_id, host_id, contract, step_created_at, evaluated_at, \
         status, reason, policy_version, rules_hash, enforced_outcome, shadow_outcome, degraded, \
         candidate, inputs, enforced, shadow, gaps FROM policy_shadow_decisions"
    };
}
pub(crate) use shadow_columns;

fn small(value: i64, what: &str) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|e| corrupt(what, e))
}

fn actor(text: &str) -> Result<Actor, StoreError> {
    parse(text, "policy actor")
}

/// One row of `policy_versions`.
#[derive(sqlx::FromRow)]
pub(crate) struct VersionRow {
    id: String,
    status: String,
    number: Option<i64>,
    draft_revision: i64,
    based_on: Option<i64>,
    rules_hash: String,
    rule_count: i64,
    locked_count: i64,
    shadow_count: i64,
    created_at: i64,
    created_by: String,
    updated_at: i64,
    updated_by: String,
    published_at: Option<i64>,
    published_by: Option<String>,
}

impl VersionRow {
    pub(crate) fn into_record(self) -> Result<PolicyVersionRecord, StoreError> {
        let status = PolicyStatus::parse(&self.status).ok_or_else(|| {
            StoreError::Backend(format!("unknown policy status {:?}", self.status))
        })?;
        Ok(PolicyVersionRecord {
            id: parse(&self.id, "policy version id")?,
            status,
            number: self
                .number
                .map(|n| small(n, "policy version number"))
                .transpose()?,
            revision: uint(self.draft_revision),
            based_on: self
                .based_on
                .map(|n| small(n, "policy base number"))
                .transpose()?,
            rules_hash: self.rules_hash,
            rule_count: small(self.rule_count, "policy rule count")?,
            locked_count: small(self.locked_count, "policy locked count")?,
            shadow_count: small(self.shadow_count, "policy shadow count")?,
            created_at: from_ms(self.created_at)?,
            created_by: actor(&self.created_by)?,
            updated_at: from_ms(self.updated_at)?,
            updated_by: actor(&self.updated_by)?,
            published_at: from_ms_opt(self.published_at)?,
            published_by: self.published_by.as_deref().map(actor).transpose()?,
        })
    }
}

/// One row of `policy_rules`.
#[derive(sqlx::FromRow)]
pub(crate) struct RuleRow {
    rule_id: String,
    ordinal: i64,
    enabled: i64,
    shadow: i64,
    locked: i64,
    body: String,
}

impl RuleRow {
    pub(crate) fn into_rule(self) -> Result<StoredRule, StoreError> {
        Ok(StoredRule {
            rule_id: self.rule_id,
            ordinal: small(self.ordinal, "policy rule ordinal")?,
            enabled: self.enabled != 0,
            shadow: self.shadow != 0,
            locked: self.locked != 0,
            body: self.body,
        })
    }
}

/// One row of `policy_acknowledgements`.
#[derive(sqlx::FromRow)]
pub(crate) struct AckRow {
    finding_key: String,
    kind: String,
    first_rule: String,
    second_rule: String,
    resolution: Option<String>,
    note: String,
    acknowledged_by: String,
    acknowledged_at: i64,
}

impl AckRow {
    pub(crate) fn into_ack(self) -> Result<AckRecord, StoreError> {
        Ok(AckRecord {
            finding_key: self.finding_key,
            kind: self.kind,
            first_rule: self.first_rule,
            second_rule: self.second_rule,
            resolution: self.resolution,
            note: self.note,
            acknowledged_by: actor(&self.acknowledged_by)?,
            acknowledged_at: from_ms(self.acknowledged_at)?,
        })
    }
}

/// One row of `policy_shadow_decisions`.
#[derive(sqlx::FromRow)]
pub(crate) struct ShadowRow {
    execution_id: String,
    operation_id: String,
    host_id: String,
    contract: String,
    step_created_at: i64,
    evaluated_at: i64,
    status: String,
    reason: Option<String>,
    policy_version: i64,
    rules_hash: String,
    enforced_outcome: Option<String>,
    shadow_outcome: Option<String>,
    degraded: i64,
    candidate: String,
    inputs: Option<String>,
    enforced: Option<String>,
    shadow: Option<String>,
    gaps: String,
}

impl ShadowRow {
    pub(crate) fn into_record(self) -> Result<ShadowRecord, StoreError> {
        let status = ShadowStatus::parse(&self.status).ok_or_else(|| {
            StoreError::Backend(format!("unknown shadow status {:?}", self.status))
        })?;
        Ok(ShadowRecord {
            execution: parse(&self.execution_id, "execution id")?,
            operation: parse(&self.operation_id, "operation id")?,
            host: parse(&self.host_id, "host id")?,
            contract: self.contract,
            step_created_at: from_ms(self.step_created_at)?,
            evaluated_at: from_ms(self.evaluated_at)?,
            status,
            reason: self.reason,
            policy_version: small(self.policy_version, "policy version")?,
            rules_hash: self.rules_hash,
            enforced_outcome: self.enforced_outcome,
            shadow_outcome: self.shadow_outcome,
            degraded: self.degraded != 0,
            candidate: self.candidate,
            inputs: self.inputs,
            enforced: self.enforced,
            shadow: self.shadow,
            gaps: self.gaps,
        })
    }
}

/// A candidate: an execution row joined with its operation's pinned subjects.
#[derive(sqlx::FromRow)]
pub(crate) struct CandidateRow {
    execution_id: String,
    operation_id: String,
    host_id: String,
    contract: String,
    contract_version: i64,
    check_key: String,
    target: Option<String>,
    args: String,
    pinned: String,
    created_at: i64,
}

impl CandidateRow {
    pub(crate) fn into_candidate(self) -> Result<ShadowCandidate, StoreError> {
        Ok(ShadowCandidate {
            execution: parse(&self.execution_id, "execution id")?,
            operation: parse(&self.operation_id, "operation id")?,
            host: parse(&self.host_id, "host id")?,
            contract: from_name::<ContractId>(&self.contract, "contract id")?,
            contract_version: small(self.contract_version, "contract version")?,
            check: from_name(&self.check_key, "check key")?,
            target: self
                .target
                .as_deref()
                .map(|target| from_name(target, "target key"))
                .transpose()?,
            args: from_json::<BTreeMap<String, String>>(&self.args, "execution arguments")?,
            pinned: from_json::<Vec<PinnedSubject>>(&self.pinned, "operation pinned subjects")?,
            created_at: from_ms(self.created_at)?,
        })
    }
}

/// The one row of `policy_shadow_state`.
#[derive(sqlx::FromRow)]
pub(crate) struct StateRow {
    last_sweep_at: Option<i64>,
    last_error: Option<String>,
    total_evaluated: i64,
    total_unevaluable: i64,
    total_skipped: i64,
}

impl StateRow {
    pub(crate) fn into_state(self) -> Result<ShadowState, StoreError> {
        Ok(ShadowState {
            last_sweep_at: from_ms_opt(self.last_sweep_at)?,
            last_error: self.last_error,
            total_evaluated: uint(self.total_evaluated),
            total_unevaluable: uint(self.total_unevaluable),
            total_skipped: uint(self.total_skipped),
        })
    }
}
