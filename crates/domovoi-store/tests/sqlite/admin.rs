//! Owner decisions committed with their audit row: both land or neither does.

use domovoi_core::id::AckRuleId;
use domovoi_core::ports::{
    AckRepo, AdminWrite, AuditDraft, AuditInspect, CommitError, SettingsRepo, StateWrite,
    Transaction, UnitOfWork,
};
use domovoi_core::{AckRule, Actor, CheckKey, UserId};
use domovoi_testkit::build::at;

use crate::common::Fresh;

fn audit(kind: &str) -> AuditDraft {
    AuditDraft::new(
        Actor::User(UserId::new()),
        at(1),
        kind.to_owned(),
        "ack_rule:x".to_owned(),
        serde_json::Value::Null,
    )
}

fn rule() -> AckRule {
    AckRule {
        id: AckRuleId::new(),
        host: None,
        check: CheckKey::parse("disk.free").unwrap(),
        target_pattern: None,
        predicates: Vec::new(),
        note: "fine".to_owned(),
        expires_at: None,
        created_at: Some(at(1)),
        created_by: Some("me".to_owned()),
    }
}

async fn audit_rows(store: &domovoi_store::Store) -> usize {
    store
        .head()
        .await
        .unwrap()
        .map_or(0, |(seq, _)| usize::try_from(seq).unwrap())
}

#[tokio::test]
async fn a_rule_and_its_audit_row_commit_together() {
    let fresh = Fresh::new().await;
    let rule = rule();
    let before = audit_rows(&fresh.store).await;
    fresh
        .store
        .commit(
            Transaction::new()
                .write(StateWrite::Admin(AdminWrite::PutAckRule(rule.clone())))
                .audit(audit("ack_rule.created")),
        )
        .await
        .unwrap();
    assert_eq!(fresh.store.list_rules().await.unwrap().len(), 1);
    assert_eq!(audit_rows(&fresh.store).await, before + 1);
}

#[tokio::test]
async fn a_refused_write_leaves_no_audit_row() {
    let fresh = Fresh::new().await;
    let before = audit_rows(&fresh.store).await;
    let err = fresh
        .store
        .commit(
            Transaction::new()
                .write(StateWrite::Admin(AdminWrite::DeleteAckRule(
                    AckRuleId::new(),
                )))
                .audit(audit("ack_rule.deleted")),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, CommitError::Constraint(_)), "{err:?}");
    assert_eq!(audit_rows(&fresh.store).await, before);
}

#[tokio::test]
async fn a_setting_is_set_and_removed_through_the_commit() {
    let fresh = Fresh::new().await;
    let write = |value| {
        Transaction::new()
            .write(StateWrite::Admin(AdminWrite::Setting {
                key: "host.a.skills".to_owned(),
                value,
            }))
            .audit(audit("skill.disabled"))
    };
    fresh
        .store
        .commit(write(Some(serde_json::json!({"disabled": ["x"]}))))
        .await
        .unwrap();
    assert!(fresh.store.get("host.a.skills").await.unwrap().is_some());
    fresh.store.commit(write(None)).await.unwrap();
    assert!(fresh.store.get("host.a.skills").await.unwrap().is_none());
}
