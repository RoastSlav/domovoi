//! Contracts for [`ActionRepo`] and [`AuditLog`].

use domovoi_core::ports::{
    ActionFilter, ActionRepo, AuditEvent, AuditFilter, AuditLog, Page, StoreError,
};
use std::collections::BTreeMap;

use domovoi_core::{
    ActionError, ActionId, ActionRequest, ActionStatus, HostId, IncidentId, PlanHash, Principal,
    Risk, UserId,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::{Value, json};

use super::{ContractError, ContractResult};
use crate::approvals::human;
use crate::build::{BuildError, T0, action, at, manifest};

/// Exercises inserts, updates, chat tokens and listing on an empty `repo`.
pub async fn action_repo_contract(repo: &dyn ActionRepo) -> ContractResult {
    let (incident, host) = (IncidentId::new(), HostId::new());
    let mut first = action(incident, host, Risk::Write)?;
    assert_eq!(repo.get(first.id()).await?, None);
    assert_eq!(repo.update(&first).await, Err(StoreError::NotFound));
    assert_eq!(
        repo.update_from(&first, "proposed").await,
        Err(StoreError::NotFound),
        "update_from also refuses a missing id",
    );
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
    assert_eq!(
        repo.update_from(&first, "proposed").await,
        Err(StoreError::Conflict(
            "the action changed meanwhile".to_owned()
        )),
        "a decision read before the approval cannot overwrite it",
    );
    assert_eq!(
        repo.get(first.id()).await?,
        Some(first.clone()),
        "and nothing was written"
    );
    assert_eq!(repo.update_from(&first, "approved").await, Ok(()));
    assert_eq!(
        repo.get(first.id()).await?,
        Some(first.clone()),
        "update_from replaces the whole request too"
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
    let status = |name: &str| ActionFilter {
        status: Some(name.to_owned()),
        ..ActionFilter::default()
    };
    let approved = repo
        .list(&status("approved"), &Page::first(10))
        .await?
        .items;
    assert_eq!(approved, vec![first]);
    assert_eq!(
        repo.list(&status("proposed"), &Page::first(10))
            .await?
            .items
            .len(),
        2
    );
    assert!(
        repo.list(&status("verified"), &Page::first(10))
            .await?
            .items
            .is_empty()
    );

    // All three were created at T0 with a one-hour window, so the boundary is
    // exclusive: the window closing exactly at the deadline is not yet closed.
    let expiring = |closes_before: i64| ActionFilter {
        expires_before: Some(at(closes_before)),
        status: Some("proposed".to_owned()),
        ..ActionFilter::default()
    };
    assert_eq!(
        repo.list(&expiring(3600), &Page::first(10))
            .await?
            .items
            .len(),
        0,
        "no window has closed at its own instant"
    );
    assert_eq!(
        repo.list(&expiring(3601), &Page::first(10))
            .await?
            .items
            .len(),
        2,
        "only still-proposed actions whose window closed"
    );

    // `first` was approved at T0 + 60 s; the other two are still proposed, unchanged since T0.
    let decided = |decided: bool, since: Option<i64>| ActionFilter {
        decided: Some(decided),
        updated_since: since.map(at),
        ..ActionFilter::default()
    };
    let cases = [
        (decided(true, None), 1, "decided"),
        (decided(false, None), 2, "still waiting"),
        (decided(true, Some(60)), 1, "changed since"),
        (decided(true, Some(61)), 0, "changed before"),
    ];
    for (filter, expected, what) in cases {
        let got = repo.list(&filter, &Page::first(10)).await?.items.len();
        assert_eq!(got, expected, "{what}");
    }
    last_run_by_skill_contract(repo).await
}

/// An action for `skill` on `host`, approved at `T0 + 1 s` and sent to the agent at
/// `dispatched_at`, or sent with the wrong plan hash so that it is rejected.
async fn sent(
    repo: &dyn ActionRepo,
    host: HostId,
    skill: &str,
    dispatched_at: i64,
    stale_plan: bool,
) -> Result<ActionRequest, ContractError> {
    let setup = |e: ActionError| ContractError::Setup(e.to_string());
    let summary = manifest(skill, Risk::Read)?.summary();
    let args = summary
        .validate_args(&BTreeMap::new())
        .map_err(BuildError::from)?;
    let mut request = ActionRequest::propose(
        IncidentId::new(),
        host,
        &summary,
        &args,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    );
    repo.insert(&request).await?;
    request
        .approve(human(UserId::new()), at(1))
        .map_err(setup)?;
    let plan = if stale_plan {
        PlanHash::compute(&summary.name, summary.manifest_hash, HostId::new(), &args)
    } else {
        request.plan_hash()
    };
    request.dispatch(plan, at(dispatched_at)).map_err(setup)?;
    repo.update(&request).await?;
    Ok(request)
}

/// When each skill last ran on a host: only requests the agent received count, the latest
/// change wins, and other hosts do not leak in.
async fn last_run_by_skill_contract(repo: &dyn ActionRepo) -> ContractResult {
    let (host, other) = (HostId::new(), HostId::new());
    assert!(
        repo.last_run_by_skill(host).await?.is_empty(),
        "nothing ran yet"
    );

    let mut restart = sent(repo, host, "restart_container", 100, false).await?;
    restart
        .complete(Ok(()), at(180))
        .map_err(|e| ContractError::Setup(e.to_string()))?;
    repo.update(&restart).await?;
    sent(repo, host, "restart_container", 150, false).await?;
    sent(repo, host, "prune_logs", 300, false).await?;
    sent(repo, host, "zpool_status", 400, true).await?;
    sent(repo, other, "restart_container", 900, false).await?;
    repo.insert(&action(IncidentId::new(), host, Risk::Read)?)
        .await?;

    let last = repo.last_run_by_skill(host).await?;
    let got: Vec<(&str, Timestamp)> = last.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(
        got,
        vec![("prune_logs", at(300)), ("restart_container", at(180))],
        "the latest change per skill; rejected and merely proposed requests do not count",
    );
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
