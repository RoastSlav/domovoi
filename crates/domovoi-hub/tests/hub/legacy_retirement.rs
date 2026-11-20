//! What survives of the legacy action path: a read-only history, no way to decide on it, and
//! old chat buttons that say they are out of date. The runtime side (the retirement at start
//! and `/readyz`) is covered beside the runtime in `src/runtime/tests/legacy_retirement.rs`.

use std::collections::BTreeSet;

use axum::http::StatusCode;
use domovoi_core::ports::{ActionRepo, InteractionSink};
use domovoi_core::{ActionRequest, ActionStatus, CardButton, InteractionReply, Risk};
use domovoi_hub::auth::interactions::STALE_CARD;
use domovoi_testkit::approvals::human;
use serde_json::json;

use crate::api_harness::{Api, api, api_with_operations};
use crate::interactions::{ALLOWED, approve, chat, press, proposed, status_of};

/// Stores a retired action of each new kind and answers them as `(expired, not run,
/// outcome unknown)`.
async fn retired(a: &Api) -> (ActionRequest, ActionRequest, ActionRequest) {
    let now = a.now();
    let mut expired = a.propose(Risk::Write).await;
    expired.expire_at_upgrade(now).unwrap();
    let mut not_run = a.propose(Risk::Write).await;
    domovoi_testkit::approvals::approve(&mut not_run, human(domovoi_core::UserId::new()), now)
        .unwrap();
    not_run
        .never_ran("retired with the old executor".to_owned(), now)
        .unwrap();
    let mut unknown = a.propose(Risk::Write).await;
    domovoi_testkit::approvals::approve(&mut unknown, human(domovoi_core::UserId::new()), now)
        .unwrap();
    unknown
        .dispatch(unknown.plan_hash_for_approval(), None, now)
        .unwrap();
    unknown
        .outcome_unknown("retired with the old executor".to_owned(), now)
        .unwrap();
    for action in [&expired, &not_run, &unknown] {
        // `propose` stored these as proposed; move the rows to their retired form.
        ActionRepo::update_from(&*a.store, action, "proposed")
            .await
            .unwrap();
    }
    (expired, not_run, unknown)
}

#[tokio::test]
async fn legacy_projection_lists_retired_rows_read_only() {
    let a = api().await;
    let login = a.admin().await;
    let (expired, not_run, unknown) = retired(&a).await;

    let listed = a.get("/api/v1/actions", &login).await;

    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text);
    let items = listed.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "{}", listed.text);
    assert!(
        items.iter().all(|item| item["legacy"] == true),
        "every action is labelled legacy: {}",
        listed.text
    );
    let statuses: BTreeSet<&str> = items
        .iter()
        .map(|item| item["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses,
        BTreeSet::from([
            expired.status().name(),
            not_run.status().name(),
            unknown.status().name()
        ])
    );
    assert_eq!(
        statuses,
        BTreeSet::from(["expired", "not_run", "outcome_unknown"])
    );

    let filtered = a.get("/api/v1/actions?status=not_run", &login).await;
    assert_eq!(filtered.body["items"].as_array().unwrap().len(), 1);
    assert_eq!(filtered.body["items"][0]["id"], not_run.id().to_string());
    let on_incident = a
        .get(&format!("/api/v1/incidents/{}", a.incident.id()), &login)
        .await;
    assert_eq!(on_incident.body["actions"].as_array().unwrap().len(), 3);
    assert!(
        on_incident.body["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["legacy"] == true)
    );
}

#[tokio::test]
async fn approve_and_deny_routes_are_gone() {
    let a = api().await;
    let login = a.admin().await;
    let action = a.propose(Risk::Write).await;

    for verb in ["approve", "deny"] {
        let reply = a
            .post(
                &format!("/api/v1/actions/{}/{verb}", action.id()),
                &login,
                json!({ "plan_hash": action.plan_hash_for_approval().to_string() }),
            )
            .await;
        assert!(
            matches!(
                reply.status,
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{verb}: {} {}",
            reply.status,
            reply.text
        );
    }
    assert_eq!(status_of(&a, &action).await, ActionStatus::Proposed);
}

#[tokio::test]
async fn old_legacy_button_press_is_refused_as_out_of_date() {
    for with_operations in [false, true] {
        let a = if with_operations {
            api_with_operations().await
        } else {
            api().await
        };
        a.admin().await;
        let c = chat(&a).await;
        let (action, token) = proposed(&a, Risk::Write).await;
        let deny = CardButton::Deny {
            action: action.id(),
            token: token.clone(),
        };

        for button in [approve(&action, &token), deny] {
            let reply = c.authorizer.deliver(press(&a, ALLOWED, button)).await;
            assert_eq!(
                reply,
                InteractionReply::Rejected {
                    reason: STALE_CARD.to_owned()
                },
                "operations on: {with_operations}"
            );
        }
        let stored = ActionRepo::get(&*a.store, action.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.status(), &ActionStatus::Proposed, "nothing changed");
        assert!(stored.approved_by().is_none());
        assert!(
            stored.chat_token_hash().is_some(),
            "the token was not spent"
        );
    }
}
