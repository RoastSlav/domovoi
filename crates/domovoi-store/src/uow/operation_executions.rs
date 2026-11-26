//! The execution rows of the operation aggregate: guarded inserts and updates, with the
//! approval encoded in the row (only a human approval is ever stored).

use domovoi_core::ports::{CommitError, ExecutionGuard, Expectation, Observed};
use domovoi_core::{ExecutionId, ExecutionRecord, Risk, StepId};
use sqlx::SqliteConnection;

use crate::codec::{int, json, ms, name, uint};

use super::operations::{conflict, refuse};
use super::{backend, map_store};

async fn execution_version(
    conn: &mut SqliteConnection,
    step: StepId,
) -> Result<Option<u64>, CommitError> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT version FROM executions WHERE execution_id = ?")
            .bind(step.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    Ok(row.map(|(version,)| uint(version)))
}

pub(super) async fn apply_execution(
    conn: &mut SqliteConnection,
    execution: &ExecutionRecord,
    expected: ExecutionGuard,
) -> Result<(), CommitError> {
    if execution.execution != ExecutionId::from_step(execution.step) {
        return Err(refuse("an execution is keyed by its step's id"));
    }
    let step: Option<(String, String)> =
        sqlx::query_as("SELECT operation_id, kind FROM steps WHERE id = ?")
            .bind(execution.step.to_string())
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    let Some((operation, kind)) = step else {
        return Err(refuse("unknown step"));
    };
    if kind != "execution" {
        return Err(refuse("an execution row needs an Execution step"));
    }
    if operation != execution.operation.to_string() {
        return Err(refuse(
            "the execution and its step belong to different operations",
        ));
    }
    let stored = execution_version(conn, execution.step).await?;
    match (expected, stored) {
        (ExecutionGuard::New, Some(version)) => Err(conflict(
            execution.operation,
            Expectation::New,
            Observed::Version(version),
        )),
        (ExecutionGuard::New, None) => insert_execution(conn, execution).await,
        (ExecutionGuard::Version(n), None) => Err(conflict(
            execution.operation,
            Expectation::Version(n),
            Observed::Missing,
        )),
        (ExecutionGuard::Version(n), Some(version)) if version != n => Err(conflict(
            execution.operation,
            Expectation::Version(n),
            Observed::Version(version),
        )),
        (ExecutionGuard::Version(n), Some(_)) => update_execution(conn, execution, n).await,
    }
}

/// The approval and dispatch columns of an execution row, encoded.
struct ExecutionColumns {
    args: String,
    plan_hash: String,
    authorization_kind: Option<&'static str>,
    approved_by: Option<String>,
    approved_channel: Option<String>,
    approved_at: Option<i64>,
    approved_plan_hash: Option<String>,
    approval_session_ref: Option<String>,
    approval_client: Option<String>,
    agent_epoch: Option<String>,
    dispatched_at: Option<i64>,
    risk: Option<&'static str>,
    chat_token_hash: Option<Vec<u8>>,
}

fn encode_execution(execution: &ExecutionRecord) -> Result<ExecutionColumns, CommitError> {
    let approval = execution.approval();
    Ok(ExecutionColumns {
        args: json(&execution.args).map_err(map_store)?,
        plan_hash: name(&execution.plan_hash).map_err(map_store)?,
        authorization_kind: execution.authorization_kind().map(|kind| kind.name()),
        approved_by: approval.map(|a| a.approval().user().to_string()),
        approved_channel: approval
            .map(|a| json(a.approval().channel()))
            .transpose()
            .map_err(map_store)?,
        approved_at: approval.map(|a| ms(a.approval().at())),
        approved_plan_hash: approval
            .map(|a| name(&a.displayed()))
            .transpose()
            .map_err(map_store)?,
        approval_session_ref: approval
            .and_then(|a| a.provenance().session_ref())
            .map(str::to_owned),
        approval_client: approval.map(|a| a.provenance().client().to_owned()),
        agent_epoch: execution.dispatched.map(|(epoch, _)| epoch.to_string()),
        dispatched_at: execution.dispatched.map(|(_, at)| ms(at)),
        risk: execution.risk().map(Risk::name),
        chat_token_hash: execution.chat_token_hash().map(|hash| hash.to_vec()),
    })
}

/// Binds the columns after `execution_id`, in the order both statements list them.
macro_rules! bind_execution {
    ($query:expr, $e:expr, $c:expr) => {
        $query
            .bind($e.operation.to_string())
            .bind($e.host.to_string())
            .bind($e.contract.as_str())
            .bind(i64::from($e.contract_version))
            .bind($e.check.as_str())
            .bind($e.target.as_ref().map(|t| t.as_str()))
            .bind($c.args)
            .bind($c.plan_hash)
            .bind(ms($e.expires_at))
            .bind($c.authorization_kind)
            .bind($c.approved_by)
            .bind($c.approved_channel)
            .bind($c.approved_at)
            .bind($c.approved_plan_hash)
            .bind($c.approval_session_ref)
            .bind($c.approval_client)
            .bind($e.waiting_since.map(ms))
            .bind($c.agent_epoch)
            .bind($c.dispatched_at)
            .bind($e.settled_at.map(ms))
            .bind(ms($e.created_at))
            .bind(ms($e.updated_at))
            .bind($c.risk)
            .bind($c.chat_token_hash)
    };
}

async fn insert_execution(
    conn: &mut SqliteConnection,
    execution: &ExecutionRecord,
) -> Result<(), CommitError> {
    const SQL: &str = "INSERT INTO executions (operation_id, host_id, contract, \
        contract_version, check_key, target, args, plan_hash, expires_at, authorization_kind, \
        approved_by, approved_channel, approved_at, approved_plan_hash, approval_session_ref, \
        approval_client, waiting_since, agent_epoch, dispatched_at, settled_at, created_at, \
        updated_at, risk, chat_token_hash, execution_id) \
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
    let c = encode_execution(execution)?;
    bind_execution!(sqlx::query(SQL), execution, c)
        .bind(execution.step.to_string())
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    write_permit(conn, execution).await
}

async fn update_execution(
    conn: &mut SqliteConnection,
    execution: &ExecutionRecord,
    version: u64,
) -> Result<(), CommitError> {
    const SQL: &str = "UPDATE executions SET operation_id = ?, host_id = ?, contract = ?, \
        contract_version = ?, check_key = ?, target = ?, args = ?, plan_hash = ?, \
        expires_at = ?, authorization_kind = ?, approved_by = ?, approved_channel = ?, \
        approved_at = ?, approved_plan_hash = ?, approval_session_ref = ?, approval_client = ?, \
        waiting_since = ?, agent_epoch = ?, dispatched_at = ?, settled_at = ?, created_at = ?, \
        updated_at = ?, risk = ?, chat_token_hash = ?, version = version + 1 WHERE execution_id = ? AND version = ?";
    let c = encode_execution(execution)?;
    bind_execution!(sqlx::query(SQL), execution, c)
        .bind(execution.step.to_string())
        .bind(int(version))
        .execute(&mut *conn)
        .await
        .map_err(backend)?;
    write_permit(conn, execution).await
}

/// Writes the permit that authorized the execution, in the same transaction as the row. A
/// permit is immutable: once stored it is never rewritten, and a second permit for the same
/// execution is a primary-key conflict.
async fn write_permit(
    conn: &mut SqliteConnection,
    execution: &ExecutionRecord,
) -> Result<(), CommitError> {
    let Some(stored) = execution.permit() else {
        return Ok(());
    };
    let id = execution.step.to_string();
    let exists: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM execution_permits WHERE execution_id = ?")
            .bind(&id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(backend)?;
    if exists.is_some() {
        return Ok(());
    }
    let permit = stored.permit();
    sqlx::query(
        "INSERT INTO execution_permits (execution_id, decision_id, policy_version, \
         ceiling_hash, plan_hash, issued_at, expires_at, decision) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(permit.decision_id())
    .bind(permit.policy_version())
    .bind(permit.ceiling_hash())
    .bind(name(&permit.plan_hash()).map_err(map_store)?)
    .bind(ms(permit.issued_at()))
    .bind(ms(permit.expires_at()))
    .bind(stored.decision())
    .execute(&mut *conn)
    .await
    .map_err(backend)?;
    for rule in permit.rule_ids() {
        sqlx::query("INSERT INTO execution_permit_rules (execution_id, rule_id) VALUES (?, ?)")
            .bind(&id)
            .bind(rule)
            .execute(&mut *conn)
            .await
            .map_err(backend)?;
    }
    Ok(())
}
