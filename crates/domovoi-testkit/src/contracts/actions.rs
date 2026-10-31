//! Contracts for [`ActionRepo`] and [`AuditLog`].

use domovoi_core::ports::{
    ActionFilter, ActionRepo, AuditEvent, AuditFilter, AuditLog, Page, StoreError,
};
use domovoi_core::{ActionId, ActionRequest, ActionStatus, HostId, IncidentId, Risk, UserId};
use serde_json::{Value, json};

use super::{ContractError, ContractResult};
use crate::approvals::human;
use crate::build::{T0, action, at};

/// Exercises inserts, updates, chat tokens and listing on an empty `repo`.
pub async fn action_repo_contract(repo: &dyn ActionRepo) -> ContractResult {
    let (incident, host) = (IncidentId::new(), HostId::new());
    let mut first = action(incident, host, Risk::Write)?;
    assert_eq!(repo.get(first.id()).await?, None);
    assert_eq!(repo.update(&first).await, Err(StoreError::NotFound));
    repo.insert(&first).await?;
    assert_eq!(repo.get(first.id()).await?, Some(first.clone()));
    assert!(matches!(
        repo.insert(&first).await,
        Err(StoreError::Conflict(_))
    ));

    first
        .approve(human(UserId::new()), at(60))
        .map_err(|e| ContractError::Setup(e.to_string()))?;
    repo.update(&first).await?;
    let stored = repo.get(first.id()).await?;
    assert_eq!(
        stored.as_ref().map(ActionRequest::status),
        Some(&ActionStatus::Approved)
    );
    assert_eq!(
        stored,
        Some(first.clone()),
        "update replaces the whole request"
    );

    let tokened = action(incident, host, Risk::Write)?.with_chat_token_hash([5; 32]);
    repo.insert(&tokened).await?;
    assert_eq!(
        repo.find_by_chat_token_hash(&[5; 32]).await?,
        Some(tokened.clone())
    );
    assert_eq!(repo.find_by_chat_token_hash(&[6; 32]).await?, None);
    let clash = action(incident, host, Risk::Write)?.with_chat_token_hash([5; 32]);
    assert!(
        matches!(repo.insert(&clash).await, Err(StoreError::Conflict(_))),
        "chat token hashes are unique"
    );

    let elsewhere = action(IncidentId::new(), HostId::new(), Risk::Read)?;
    repo.insert(&elsewhere).await?;

    // All three were created at T0, so ids break the tie, newest first.
    let mut expected: Vec<ActionId> = vec![first.id(), tokened.id(), elsewhere.id()];
    expected.sort_by(|a, b| b.cmp(a));
    let mut seen = Vec::new();
    let mut page = Page::first(1);
    loop {
        let result = repo.list(&ActionFilter::default(), &page).await?;
        seen.extend(result.items.iter().map(ActionRequest::id));
        match result.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen, expected, "newest first, no gaps or repeats");
    assert_eq!(first.created_at(), T0);

    let by_incident = ActionFilter {
        incident: Some(incident),
        ..ActionFilter::default()
    };
    assert_eq!(
        repo.list(&by_incident, &Page::first(10)).await?.items.len(),
        2
    );
    let by_host = ActionFilter {
        host: Some(elsewhere.host()),
        ..ActionFilter::default()
    };
    let only = repo.list(&by_host, &Page::first(10)).await?.items;
    assert_eq!(only, vec![elsewhere]);
    Ok(())
}

fn event(actor: &str, kind: &str, subject: &str, secs: i64) -> AuditEvent {
    AuditEvent {
        at: at(secs),
        actor: actor.to_owned(),
        kind: kind.to_owned(),
        subject: subject.to_owned(),
        detail: if secs % 2 == 0 {
            Value::Null
        } else {
            json!({ "secs": secs })
        },
    }
}

/// Exercises appending, sequence numbers, filters and paging on an empty `log`.
pub async fn audit_log_contract(log: &dyn AuditLog) -> ContractResult {
    let events = [
        event("user:a", "action.approved", "action:1", 1),
        event("system:triage", "action.proposed", "action:1", 2),
        event("user:a", "incident.acked", "incident:9", 3),
        event("chat:discord:42", "action.approved", "action:2", 4),
    ];
    for (expected, e) in (1..).zip(&events) {
        assert_eq!(
            log.append(e).await?,
            expected,
            "sequence numbers count up from 1"
        );
    }

    let all = log.list(&AuditFilter::default(), &Page::first(10)).await?;
    let newest_first: Vec<AuditEvent> = events.iter().rev().cloned().collect();
    assert_eq!(all.items, newest_first, "newest first, stored unchanged");
    assert_eq!(all.next_cursor, None);

    let mut seen = Vec::new();
    let mut page = Page::first(3);
    loop {
        let result = log.list(&AuditFilter::default(), &page).await?;
        seen.extend(result.items);
        if seen.len() == 3 {
            log.append(&event("user:b", "late.event", "x", 9)).await?;
        }
        match result.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => break,
        }
    }
    assert_eq!(seen, newest_first, "pages are stable while the log grows");

    let filters = [
        (
            AuditFilter {
                actor: Some("user:a".into()),
                ..AuditFilter::default()
            },
            2,
        ),
        (
            AuditFilter {
                kind: Some("action.approved".into()),
                ..AuditFilter::default()
            },
            2,
        ),
        (
            AuditFilter {
                subject: Some("action:1".into()),
                ..AuditFilter::default()
            },
            2,
        ),
        (
            AuditFilter {
                since: Some(at(3)),
                ..AuditFilter::default()
            },
            3,
        ),
        (
            AuditFilter {
                actor: Some("user:a".into()),
                kind: Some("action.approved".into()),
                ..AuditFilter::default()
            },
            1,
        ),
        (
            AuditFilter {
                actor: Some("user".into()),
                ..AuditFilter::default()
            },
            0,
        ),
    ];
    for (filter, expected) in filters {
        let got = log.list(&filter, &Page::first(100)).await?.items.len();
        assert_eq!(got, expected, "{filter:?}");
    }
    assert_eq!(log.append(&event("user:c", "x", "y", 10)).await?, 6);
    Ok(())
}
