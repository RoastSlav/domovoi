//! Contracts for stored policy: drafts, publication, acknowledgements and shadow decisions,
//! run against the in-memory store and SQLite.

use domovoi_core::policy_store::{AckRecord, DraftGuard, PolicyStatus, PolicyWrite, StoredRule};
use domovoi_core::ports::{CommitError, Page, StateWrite, Transaction};
use domovoi_core::{Actor, PolicyVersionId};

use super::{ContractResult, UowSubject};
use crate::build::at;

pub(super) const HASH: &str = "abababababababababababababababababababababababababababababababab";

pub(super) fn missing(what: &str) -> crate::ContractError {
    crate::ContractError::Setup(format!("expected {what}"))
}

fn actor() -> Actor {
    Actor::System("test".to_owned())
}

fn rule(id: &str, ordinal: u32, locked: bool, shadow: bool) -> StoredRule {
    StoredRule {
        rule_id: id.to_owned(),
        ordinal,
        enabled: true,
        shadow,
        locked,
        body: format!("{{\"v\":1,\"rule\":{{\"id\":\"{id}\"}}}}"),
    }
}

pub(super) async fn write(
    subject: &dyn UowSubject,
    write: PolicyWrite,
) -> Result<domovoi_core::ports::CommitReceipt, CommitError> {
    subject
        .uow()
        .commit(Transaction::new().write(StateWrite::Policy(write)))
        .await
}

fn save(
    id: PolicyVersionId,
    expected: DraftGuard,
    rules: Vec<StoredRule>,
    secs: i64,
) -> PolicyWrite {
    PolicyWrite::SaveDraft {
        id,
        expected,
        based_on: None,
        rules,
        rules_hash: HASH.to_owned(),
        actor: actor(),
        at: at(secs),
    }
}

fn publish(id: PolicyVersionId, revision: u64, number: u32, acks: Vec<AckRecord>) -> PolicyWrite {
    PolicyWrite::Publish {
        id,
        expected_revision: revision,
        number,
        acknowledgements: acks,
        actor: actor(),
        at: at(500),
    }
}

fn is_conflict<T>(outcome: &Result<T, CommitError>) -> bool {
    matches!(outcome, Err(CommitError::Conflict { .. }))
}

/// A draft is created, replaced under a revision guard and discarded with its rules.
pub async fn draft_create_replace_discard_round_trip(subject: &dyn UowSubject) -> ContractResult {
    let id = PolicyVersionId::new();
    let rules = vec![rule("a", 0, true, false), rule("b", 1, false, true)];
    write(subject, save(id, DraftGuard::Absent, rules, 10)).await?;
    let draft = subject
        .policy()
        .draft()
        .await?
        .ok_or_else(|| missing("a draft"))?;
    assert_eq!(draft.id, id);
    assert_eq!(draft.status, PolicyStatus::Draft);
    assert_eq!(draft.revision, 1);
    assert_eq!(draft.number, None);
    assert_eq!(
        (draft.rule_count, draft.locked_count, draft.shadow_count),
        (2, 1, 1)
    );

    let replaced = vec![
        rule("c", 0, false, false),
        rule("d", 1, false, false),
        rule("e", 2, false, false),
    ];
    write(
        subject,
        save(id, DraftGuard::Revision(1), replaced.clone(), 20),
    )
    .await?;
    let draft = subject
        .policy()
        .draft()
        .await?
        .ok_or_else(|| missing("a draft"))?;
    assert_eq!(draft.revision, 2);
    assert_eq!(draft.rule_count, 3);
    assert_eq!(draft.updated_at, at(20));
    assert_eq!(draft.created_at, at(10));
    assert_eq!(subject.policy().rules(id).await?, replaced);

    write(
        subject,
        PolicyWrite::DiscardDraft {
            id,
            expected_revision: 2,
        },
    )
    .await?;
    assert!(subject.policy().draft().await?.is_none());
    assert!(subject.policy().by_id(id).await?.is_none());
    assert!(subject.policy().rules(id).await?.is_empty());
    Ok(())
}

/// Only one draft may exist.
pub async fn second_draft_is_refused(subject: &dyn UowSubject) -> ContractResult {
    let first = PolicyVersionId::new();
    write(subject, save(first, DraftGuard::Absent, vec![], 10)).await?;
    let second = write(
        subject,
        save(PolicyVersionId::new(), DraftGuard::Absent, vec![], 11),
    )
    .await;
    assert!(is_conflict(&second));
    assert_eq!(
        subject
            .policy()
            .draft()
            .await?
            .ok_or_else(|| missing("draft"))?
            .id,
        first
    );
    Ok(())
}

/// A save against an old revision changes nothing.
pub async fn stale_draft_revision_is_refused(subject: &dyn UowSubject) -> ContractResult {
    let id = PolicyVersionId::new();
    write(
        subject,
        save(id, DraftGuard::Absent, vec![rule("a", 0, false, false)], 10),
    )
    .await?;
    let newer = vec![rule("b", 0, false, false)];
    write(
        subject,
        save(id, DraftGuard::Revision(1), newer.clone(), 11),
    )
    .await?;
    let stale = write(
        subject,
        save(
            id,
            DraftGuard::Revision(1),
            vec![rule("z", 0, false, false)],
            12,
        ),
    )
    .await;
    assert!(is_conflict(&stale));
    assert_eq!(subject.policy().rules(id).await?, newer);
    assert_eq!(
        subject
            .policy()
            .draft()
            .await?
            .ok_or_else(|| missing("draft"))?
            .revision,
        2
    );
    Ok(())
}

/// Publishing numbers versions 1, 2, … and freezes their rules.
pub async fn publish_assigns_consecutive_numbers_and_freezes_rules(
    subject: &dyn UowSubject,
) -> ContractResult {
    let first = PolicyVersionId::new();
    let first_rules = vec![rule("a", 0, false, false)];
    write(
        subject,
        save(first, DraftGuard::Absent, first_rules.clone(), 10),
    )
    .await?;
    write(subject, publish(first, 1, 1, vec![])).await?;
    let active = subject
        .policy()
        .active()
        .await?
        .ok_or_else(|| missing("active"))?;
    assert_eq!(active.id, first);
    assert_eq!(active.number, Some(1));
    assert_eq!(active.status, PolicyStatus::Published);
    assert_eq!(active.published_at, Some(at(500)));
    assert!(subject.policy().draft().await?.is_none());

    let second = PolicyVersionId::new();
    write(
        subject,
        save(
            second,
            DraftGuard::Absent,
            vec![rule("b", 0, false, false)],
            20,
        ),
    )
    .await?;
    write(subject, publish(second, 1, 2, vec![])).await?;
    let active = subject
        .policy()
        .active()
        .await?
        .ok_or_else(|| missing("active"))?;
    assert_eq!((active.id, active.number), (second, Some(2)));
    assert_eq!(subject.policy().rules(first).await?, first_rules);
    assert_eq!(
        subject
            .policy()
            .version(1)
            .await?
            .ok_or_else(|| missing("v1"))?
            .id,
        first
    );
    let listed = subject.policy().versions(&Page::first(10)).await?;
    let numbers: Vec<_> = listed.items.iter().map(|v| v.number).collect();
    assert_eq!(numbers, vec![Some(2), Some(1)]);
    Ok(())
}

/// A publication that skips a number is refused and keeps the draft.
pub async fn publish_with_wrong_number_conflicts(subject: &dyn UowSubject) -> ContractResult {
    let first = PolicyVersionId::new();
    write(subject, save(first, DraftGuard::Absent, vec![], 10)).await?;
    write(subject, publish(first, 1, 1, vec![])).await?;
    let second = PolicyVersionId::new();
    write(subject, save(second, DraftGuard::Absent, vec![], 20)).await?;
    let wrong = write(subject, publish(second, 1, 5, vec![])).await;
    assert!(is_conflict(&wrong));
    let draft = subject
        .policy()
        .draft()
        .await?
        .ok_or_else(|| missing("draft kept"))?;
    assert_eq!(draft.id, second);
    assert_eq!(
        subject
            .policy()
            .active()
            .await?
            .ok_or_else(|| missing("active"))?
            .number,
        Some(1)
    );
    Ok(())
}

/// Acknowledgements are stored with the version, ordered by key.
pub async fn acknowledgements_are_stored_with_the_version(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = PolicyVersionId::new();
    write(subject, save(id, DraftGuard::Absent, vec![], 10)).await?;
    let ack = |key: &str, resolution: Option<&str>| AckRecord {
        finding_key: key.to_owned(),
        kind: "incomparable_conflict".to_owned(),
        first_rule: "a".to_owned(),
        second_rule: "b".to_owned(),
        resolution: resolution.map(str::to_owned),
        note: "reviewed".to_owned(),
        acknowledged_by: actor(),
        acknowledged_at: at(400),
    };
    let acks = vec![ack("b-key", Some("deny")), ack("a-key", None)];
    write(subject, publish(id, 1, 1, acks)).await?;
    let stored = subject.policy().acknowledgements(id).await?;
    assert_eq!(stored, vec![ack("a-key", None), ack("b-key", Some("deny"))]);
    Ok(())
}
