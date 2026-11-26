//! Contract for [`AuditLog`].

use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, Page};
use serde_json::{Value, json};

use super::ContractResult;
use crate::build::at;

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
