//! Reading permits, the pause switch and the circuit breaker's inputs.

use async_trait::async_trait;
use domovoi_core::ports::{AutonomyRow, PermitRepo, RuleFailures, StoreError, UnresolvedUnknown};
use domovoi_core::{ContractId, HostId, TargetKey};
use jiff::Timestamp;

use crate::codec::{from_ms, from_ms_opt, ms, parse};
use crate::db::Store;
use crate::error::db;

#[async_trait]
impl PermitRepo for Store {
    async fn autonomy(&self) -> Result<AutonomyRow, StoreError> {
        let row: (i64, Option<String>, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT paused, changed_by, changed_at, reason FROM autonomy_state WHERE id = 1",
        )
        .fetch_one(&self.read)
        .await
        .map_err(db)?;
        Ok(AutonomyRow {
            paused: row.0 != 0,
            changed_by: row.1,
            changed_at: from_ms_opt(row.2)?,
            reason: row.3,
        })
    }

    async fn permits_issued_since(
        &self,
        since: Timestamp,
        scope: Option<(HostId, &ContractId)>,
    ) -> Result<u32, StoreError> {
        let count: (i64,) = match scope {
            None => sqlx::query_as("SELECT count(*) FROM execution_permits WHERE issued_at >= ?")
                .bind(ms(since))
                .fetch_one(&self.read)
                .await
                .map_err(db)?,
            Some((host, contract)) => sqlx::query_as(
                "SELECT count(*) FROM execution_permits p JOIN executions e \
                 ON e.execution_id = p.execution_id \
                 WHERE p.issued_at >= ? AND e.host_id = ? AND e.contract = ?",
            )
            .bind(ms(since))
            .bind(host.to_string())
            .bind(contract.as_str())
            .fetch_one(&self.read)
            .await
            .map_err(db)?,
        };
        Ok(u32::try_from(count.0).unwrap_or(u32::MAX))
    }

    async fn rule_permits_since(&self, rule: &str, since: Timestamp) -> Result<u32, StoreError> {
        let count: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM execution_permit_rules r JOIN execution_permits p \
             ON p.execution_id = r.execution_id WHERE r.rule_id = ? AND p.issued_at >= ?",
        )
        .bind(rule)
        .bind(ms(since))
        .fetch_one(&self.read)
        .await
        .map_err(db)?;
        Ok(u32::try_from(count.0).unwrap_or(u32::MAX))
    }

    async fn rule_failures(
        &self,
        window_start: Timestamp,
    ) -> Result<Vec<RuleFailures>, StoreError> {
        // A failure counts after the later of the window start and the rule's last reset.
        const SQL: &str = "SELECT r.rule_id, count(*), min(e.settled_at) \
            FROM execution_permit_rules r \
            JOIN executions e ON e.execution_id = r.execution_id \
            JOIN steps s ON s.id = e.execution_id \
            WHERE s.status IN ('failed', 'outcome_unknown') \
              AND e.settled_at IS NOT NULL \
              AND e.settled_at >= max(?, COALESCE((SELECT max(reset_at) \
                    FROM rule_circuit_resets c WHERE c.rule_id = r.rule_id), 0)) \
            GROUP BY r.rule_id ORDER BY r.rule_id";
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(SQL)
            .bind(ms(window_start))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter()
            .map(|(rule, failures, since)| {
                Ok(RuleFailures {
                    rule,
                    failures: u32::try_from(failures).unwrap_or(u32::MAX),
                    since: from_ms(since)?,
                })
            })
            .collect()
    }

    async fn unresolved_unknown(
        &self,
        host: HostId,
        contract: &ContractId,
        target: &TargetKey,
    ) -> Result<Option<UnresolvedUnknown>, StoreError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT e.execution_id FROM executions e JOIN steps s ON s.id = e.execution_id \
             WHERE e.host_id = ? AND e.contract = ? AND e.target = ? \
               AND s.status = 'outcome_unknown' \
               AND NOT EXISTS (SELECT 1 FROM unknown_resolutions u \
                               WHERE u.execution_id = e.execution_id) \
             ORDER BY e.created_at DESC LIMIT 1",
        )
        .bind(host.to_string())
        .bind(contract.as_str())
        .bind(target.as_str())
        .fetch_optional(&self.read)
        .await
        .map_err(db)?;
        row.map(|(id,)| {
            let step = parse(&id, "step id")?;
            Ok(UnresolvedUnknown {
                execution: domovoi_core::ExecutionId::from_step(step),
                step,
            })
        })
        .transpose()
    }
}
