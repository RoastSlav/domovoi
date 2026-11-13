//! The unit-of-work contracts against SQLite, and what SQLite stores beyond them.

use async_trait::async_trait;
use domovoi_core::EffectId;
use domovoi_core::ports::{
    ActionRepo, AuditLog, ClaimToken, EffectRead, Expected, IncidentRepo, IncidentTimelineEntry,
    JournalRead, NoteRepo, SignalRepo, StateWrite, Transaction, UnitOfWork, VersionedRead,
};
use domovoi_store::Store;
use domovoi_testkit::ContractError;
use domovoi_testkit::build::{at, host, open_incident};
use domovoi_testkit::contracts::UowSubject;
use jiff::Timestamp;

use crate::common;

struct SqliteSubject(Store);

#[async_trait]
impl UowSubject for SqliteSubject {
    fn uow(&self) -> &dyn UnitOfWork {
        &self.0
    }

    fn versions(&self) -> &dyn VersionedRead {
        &self.0
    }

    fn journal(&self) -> &dyn JournalRead {
        &self.0
    }

    fn effects(&self) -> &dyn EffectRead {
        &self.0
    }

    fn incidents(&self) -> &dyn IncidentRepo {
        &self.0
    }

    fn signals(&self) -> &dyn SignalRepo {
        &self.0
    }

    fn actions(&self) -> &dyn ActionRepo {
        &self.0
    }

    fn notes(&self) -> &dyn NoteRepo {
        &self.0
    }

    fn audit(&self) -> &dyn AuditLog {
        &self.0
    }

    async fn force_claim(
        &self,
        id: EffectId,
        token: ClaimToken,
        lease_until: Timestamp,
    ) -> Result<(), ContractError> {
        let done = sqlx::query(
            "UPDATE effects SET status = 'claimed', claim_token = ?, \
             claim_generation = claim_generation + 1, lease_until = ?, attempt = attempt + 1 \
             WHERE id = ?",
        )
        .bind(token.to_string())
        .bind(lease_until.as_millisecond())
        .bind(id.to_string())
        .execute(self.0.pool_for_tests())
        .await
        .map_err(|e| ContractError::Setup(e.to_string()))?;
        if done.rows_affected() == 0 {
            return Err(ContractError::Setup("the effect is missing".to_owned()));
        }
        Ok(())
    }
}

macro_rules! uow_contract_tests {
    ($($name:ident => $suite:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $name() {
                let fresh = common::Fresh::new().await;
                let subject = SqliteSubject(fresh.store.clone());
                domovoi_testkit::contracts::$suite(&subject).await.unwrap();
            }
        )*
    };
}

uow_contract_tests! {
    sqlite_commit_applies_state_journal_audit_and_effects_together => commit_applies_state_journal_audit_and_effects_together,
    sqlite_empty_transaction_commits_nothing => empty_transaction_commits_nothing,
    sqlite_stale_incident_version_conflicts_and_changes_nothing => stale_incident_version_conflicts_and_changes_nothing,
    sqlite_new_incident_over_existing_id_conflicts => new_incident_over_existing_id_conflicts,
    sqlite_second_active_incident_for_a_fingerprint_is_refused_and_rolls_back => second_active_incident_for_a_fingerprint_is_refused_and_rolls_back,
    sqlite_later_failure_rolls_back_earlier_writes_journal_audit_and_effects => later_failure_rolls_back_earlier_writes_journal_audit_and_effects,
    sqlite_repository_writes_advance_versions_like_commits => repository_writes_advance_versions_like_commits,
    sqlite_action_guard_refuses_a_changed_status => action_guard_refuses_a_changed_status,
    sqlite_chat_token_hash_must_be_unique_across_actions => chat_token_hash_must_be_unique_across_actions,
    sqlite_incident_event_is_mirrored_into_the_journal_with_its_original_time => incident_event_is_mirrored_into_the_journal_with_its_original_time,
    sqlite_journal_sequence_follows_commit_order => journal_sequence_follows_commit_order,
    sqlite_discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals => discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals,
    sqlite_audit_draft_is_appended_with_the_next_sequence_number => audit_draft_is_appended_with_the_next_sequence_number,
    sqlite_duplicate_dedup_key_is_refused_while_pending => duplicate_dedup_key_is_refused_while_pending,
    sqlite_effect_completion_needs_the_current_claim_token => effect_completion_needs_the_current_claim_token,
    sqlite_completing_an_unclaimed_effect_without_a_token_marks_it_done => completing_an_unclaimed_effect_without_a_token_marks_it_done,
    sqlite_zero_max_attempts_is_refused => zero_max_attempts_is_refused,
}

#[tokio::test]
async fn timeline_write_stores_legacy_event_seq() {
    let fresh = common::Fresh::new().await;
    let host = host("nas").unwrap();
    let open = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    IncidentRepo::insert(&fresh.store, &open).await.unwrap();
    let entry = IncidentTimelineEntry {
        incident: open.id(),
        at: at(500),
        kind: "status.changed".to_owned(),
        summary: "opened".to_owned(),
        detail: serde_json::Value::Null,
    };
    fresh
        .store
        .commit(Transaction::new().write(StateWrite::IncidentEvent(entry)))
        .await
        .unwrap();

    let legacy: (Option<i64>,) =
        sqlx::query_as("SELECT legacy_event_seq FROM journal WHERE kind = 'incident.timeline'")
            .fetch_one(fresh.store.pool_for_tests())
            .await
            .unwrap();
    let event: (i64,) = sqlx::query_as("SELECT seq FROM incident_events")
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(legacy.0, Some(event.0));
}

#[tokio::test]
async fn incident_body_carries_a_schema_version() {
    let fresh = common::Fresh::new().await;
    let host = host("nas").unwrap();
    let via_repo = open_incident(host.id, "disk.smart", Some("/dev/sda")).unwrap();
    IncidentRepo::insert(&fresh.store, &via_repo).await.unwrap();

    let via_commit = open_incident(host.id, "load.high", None).unwrap();
    fresh
        .store
        .commit(Transaction::new().write(StateWrite::Incident {
            incident: via_commit.clone(),
            expected: Expected::New,
        }))
        .await
        .unwrap();

    for id in [via_repo.id(), via_commit.id()] {
        let body: (String,) = sqlx::query_as("SELECT body FROM incidents WHERE id = ?")
            .bind(id.to_string())
            .fetch_one(fresh.store.pool_for_tests())
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&body.0).unwrap();
        assert_eq!(value["schema_version"], 1);
    }

    // A body written before the key existed still loads.
    let body: (String,) = sqlx::query_as("SELECT body FROM incidents WHERE id = ?")
        .bind(via_repo.id().to_string())
        .fetch_one(fresh.store.pool_for_tests())
        .await
        .unwrap();
    let mut old: serde_json::Value = serde_json::from_str(&body.0).unwrap();
    old.as_object_mut().unwrap().remove("schema_version");
    sqlx::query("UPDATE incidents SET body = ? WHERE id = ?")
        .bind(old.to_string())
        .bind(via_repo.id().to_string())
        .execute(fresh.store.pool_for_tests())
        .await
        .unwrap();
    assert!(
        IncidentRepo::get(&fresh.store, via_repo.id())
            .await
            .unwrap()
            .is_some()
    );
}
