//! Retention: deleting what the policy no longer keeps. Incidents, the audit log and
//! digest facts are never purged.

use domovoi_core::ports::StoreError;
use domovoi_core::triage::{CLEAR_KIND, FOLLOW_UP_KIND};
use jiff::{SignedDuration, Timestamp};

use crate::codec::ms;
use crate::db::Store;
use crate::error::db;

/// Tables `purge` deletes from. The list is the contract the full-text guard checks: a
/// table added here that gains a full-text index must also be listed in [`FTS_CLEANUP`].
pub const PURGED_TABLES: &[&str] = &[
    "signals",
    "incident_events",
    "notify_state",
    "triage_cache",
    "advisories",
    "maintenance_windows",
    "sessions",
    "messages",
    "conversations",
];

/// Purged tables that have a full-text index, with the statement that removes the index
/// rows. Run inside the same transaction, before the table's own delete. Empty today: no
/// purged table has a full-text index. A table added to [`PURGED_TABLES`] that gains an
/// index must be listed here.
pub const FTS_CLEANUP: &[(&str, &str)] = &[];

/// How long each kind of record is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Signals, by when they were observed.
    pub signals: SignedDuration,
    /// Incident timeline entries other than follow-up conversations, by when they
    /// happened.
    pub timeline: SignedDuration,
    /// Follow-up questions, answers and their clear markers, by when they happened.
    pub conversations: SignedDuration,
    /// The notification state of resolved incidents, by when it was last updated.
    pub notifications: SignedDuration,
    /// Cached triage reports, by when they were stored.
    pub triage: SignedDuration,
    /// Advisories, from when they were resolved.
    pub resolved_advisories: SignedDuration,
    /// Content of retracted facts and relations, from when they were retracted. The
    /// tombstone stays.
    pub retracted_facts: SignedDuration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            signals: SignedDuration::from_hours(24 * 30),
            timeline: SignedDuration::from_hours(24 * 90),
            conversations: SignedDuration::from_hours(24 * 180),
            notifications: SignedDuration::from_hours(24 * 30),
            triage: SignedDuration::from_hours(24 * 90),
            resolved_advisories: SignedDuration::from_hours(24 * 30),
            retracted_facts: SignedDuration::from_hours(24 * 30),
        }
    }
}

/// How many rows one purge deleted, per table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Signals.
    pub signals: u64,
    /// Incident timeline entries other than follow-up conversations.
    pub timeline: u64,
    /// Follow-up questions and answers, and their clear markers.
    pub conversations: u64,
    /// Notification state of resolved incidents.
    pub notifications: u64,
    /// Cached triage reports.
    pub triage: u64,
    /// Resolved advisories.
    pub advisories: u64,
    /// Maintenance windows that have ended.
    pub windows: u64,
    /// Expired web sessions.
    pub sessions: u64,
    /// Retracted facts and relations whose content was cleared.
    pub retracted_facts: u64,
    /// Operator conversation messages.
    pub operator_messages: u64,
    /// Operator conversations left empty and not attached to an operation.
    pub operator_conversations: u64,
}

const DELETE_SIGNALS: &str = "DELETE FROM signals WHERE observed_at < ?";
const DELETE_TIMELINE: &str = "DELETE FROM incident_events WHERE at < ? AND kind NOT IN (?, ?)";
const DELETE_CONVERSATIONS: &str = "DELETE FROM incident_events WHERE at < ? AND kind IN (?, ?)";
const DELETE_NOTIFICATIONS: &str = "DELETE FROM notify_state WHERE updated_at < ? AND held = 0 \
    AND incident_id IN (SELECT id FROM incidents WHERE status = 'resolved')";
const DELETE_TRIAGE: &str = "DELETE FROM triage_cache WHERE stored_at < ?";
const DELETE_ADVISORIES: &str =
    "DELETE FROM advisories WHERE resolved_at IS NOT NULL AND resolved_at < ?";
const DELETE_WINDOWS: &str = "DELETE FROM maintenance_windows WHERE ends_at <= ?";
const CLEAR_FACTS: &str = "UPDATE facts SET value = NULL, search_text = NULL, live = 0,     content_purged_at = ?1, version = version + 1     WHERE status = 'retracted' AND retracted_at < ?2 AND content_purged_at IS NULL";
const CLEAR_RELATIONS: &str = "UPDATE relations SET from_ref = NULL, to_ref = NULL, live = 0,     content_purged_at = ?1, version = version + 1     WHERE status = 'retracted' AND retracted_at < ?2 AND content_purged_at IS NULL";
const DELETE_OPERATOR_MESSAGES: &str =
    "DELETE FROM messages WHERE created_at < ? AND status <> 'pending'";
const DELETE_OPERATOR_CONVERSATIONS: &str = "DELETE FROM conversations WHERE updated_at < ?     AND operation_id IS NULL     AND NOT EXISTS (SELECT 1 FROM messages WHERE messages.conversation_id = conversations.id)";
const DELETE_SESSIONS: &str = "DELETE FROM sessions WHERE expires_at <= ?";

fn cutoff(now: Timestamp, keep: SignedDuration) -> i64 {
    ms(now.checked_sub(keep).unwrap_or(Timestamp::MIN))
}

/// How long a purged table keeps rows: `None` means it is cut off at `now`.
fn keep_for(table: &str, policy: &RetentionPolicy) -> Option<SignedDuration> {
    match table {
        "signals" => Some(policy.signals),
        "incident_events" => Some(policy.timeline),
        "notify_state" => Some(policy.notifications),
        "triage_cache" => Some(policy.triage),
        "advisories" => Some(policy.resolved_advisories),
        "messages" | "conversations" => Some(policy.conversations),
        _ => None,
    }
}

impl Store {
    /// Deletes everything `policy` no longer keeps at `now`.
    ///
    /// Every statement runs in one transaction on the writer connection: a failure rolls
    /// all of them back and returns the error, so nothing is deleted.
    pub async fn purge(
        &self,
        now: Timestamp,
        policy: &RetentionPolicy,
    ) -> Result<PurgeReport, StoreError> {
        let mut tx = self.write.begin().await.map_err(db)?;
        for (table, sql) in FTS_CLEANUP.iter().copied() {
            let before = keep_for(table, policy).map_or_else(|| ms(now), |keep| cutoff(now, keep));
            sqlx::query(sql)
                .bind(before)
                .execute(&mut *tx)
                .await
                .map_err(db)?;
        }
        let signals = sqlx::query(DELETE_SIGNALS)
            .bind(cutoff(now, policy.signals))
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let timeline = sqlx::query(DELETE_TIMELINE)
            .bind(cutoff(now, policy.timeline))
            .bind(FOLLOW_UP_KIND)
            .bind(CLEAR_KIND)
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let conversations = sqlx::query(DELETE_CONVERSATIONS)
            .bind(cutoff(now, policy.conversations))
            .bind(FOLLOW_UP_KIND)
            .bind(CLEAR_KIND)
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let notifications = sqlx::query(DELETE_NOTIFICATIONS)
            .bind(cutoff(now, policy.notifications))
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let triage = sqlx::query(DELETE_TRIAGE)
            .bind(cutoff(now, policy.triage))
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let advisories = sqlx::query(DELETE_ADVISORIES)
            .bind(cutoff(now, policy.resolved_advisories))
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let windows = sqlx::query(DELETE_WINDOWS)
            .bind(ms(now))
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let sessions = sqlx::query(DELETE_SESSIONS)
            .bind(ms(now))
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let retracted_before = cutoff(now, policy.retracted_facts);
        let mut retracted_facts = 0;
        for sql in [CLEAR_FACTS, CLEAR_RELATIONS] {
            retracted_facts += sqlx::query(sql)
                .bind(ms(now))
                .bind(retracted_before)
                .execute(&mut *tx)
                .await
                .map_err(db)?
                .rows_affected();
        }
        let operator_before = cutoff(now, policy.conversations);
        let operator_messages = sqlx::query(DELETE_OPERATOR_MESSAGES)
            .bind(operator_before)
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        let operator_conversations = sqlx::query(DELETE_OPERATOR_CONVERSATIONS)
            .bind(operator_before)
            .execute(&mut *tx)
            .await
            .map_err(db)?
            .rows_affected();
        tx.commit().await.map_err(db)?;
        let report = PurgeReport {
            signals,
            timeline,
            conversations,
            notifications,
            triage,
            advisories,
            windows,
            sessions,
            retracted_facts,
            operator_messages,
            operator_conversations,
        };
        tracing::info!(?report, "purged old records");
        Ok(report)
    }
}
