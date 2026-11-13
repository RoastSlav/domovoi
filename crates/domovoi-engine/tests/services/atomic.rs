//! Every action write is one commit: the action row, its audit entry and its journal
//! entry together, with the status guard still refusing a stale decision.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{
    ActionRepo, AggregateRef, AuditFilter, AuditLog, JournalKind, JournalRead, Page, SkillOutput,
    StateWrite, Transaction,
};
use domovoi_core::{
    ActionError, ActionRequest, ActionStatus, PlanHash, Principal, SignalStatus, UserId,
};
use domovoi_engine::actions::audit;
use domovoi_engine::triage::{mint_token, token_hash};
use domovoi_engine::{ActionSettings, ApprovalError, EventKind};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::approvals::{chat, human};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use serde_json::json;

use crate::actions_harness::{
    actions, actions_with_uow, check, ok_output, seeded, verified_restart,
};
use crate::harness::host_id;
use crate::uow_probe::{RecordingUow, RefusingAuditUow, StaleRaceUow};

/// The action a transaction writes.
fn written(tx: &Transaction) -> &ActionRequest {
    assert_eq!(tx.writes.len(), 1, "one state write per transition");
    match &tx.writes[0] {
        StateWrite::Action { action, .. } => action,
        other => panic!("a transaction with a non-action write: {other:?}"),
    }
}

/// Asserts the whole shape of one action commit.
fn assert_transition(tx: &Transaction, from: Option<&str>, to: &str) {
    let action = written(tx);
    assert_eq!(action.status().name(), to, "the written status");
    assert_eq!(tx.audit.len(), 1, "one audit entry per transition");
    assert_eq!(tx.journal.len(), 1, "one journal entry per transition");
    let journal = &tx.journal[0];
    assert_eq!(journal.aggregate, AggregateRef::action(action.id()));
    assert_eq!(journal.kind, JournalKind::ActionTransition);
    assert_eq!(journal.payload_v, 1);
    assert_eq!(journal.payload["action"], action.id().to_string());
    assert_eq!(journal.payload["incident"], action.incident().to_string());
    assert_eq!(journal.payload["host"], action.host().to_string());
    assert_eq!(journal.payload["skill"], action.skill().as_str());
    assert_eq!(journal.payload["from"], json!(from));
    assert_eq!(journal.payload["to"], to);
    assert_eq!(journal.payload["plan_hash"], action.plan_hash().to_string());
    assert_eq!(
        journal.actor, tx.audit[0].actor,
        "the audit and journal entries name the same actor",
    );
    assert_eq!(
        journal.occurred_at, tx.audit[0].at,
        "the audit and journal entries carry the same instant",
    );
}

async fn journal_rows(store: &InMemoryStore, id: domovoi_core::ActionId) -> Vec<String> {
    JournalRead::entries_for(store, &AggregateRef::action(id), 50)
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.kind)
        .collect()
}

/// A proposed action with a chat token, stored directly so a failing commit is the only
/// thing under test.
fn stored_proposal(incident: domovoi_core::IncidentId) -> ActionRequest {
    domovoi_testkit::build::action(incident, host_id(), domovoi_core::Risk::Write)
        .unwrap()
        .with_chat_token_hash([0x11; 32])
}

#[tokio::test]
async fn audit_failure_rolls_back_transition() {
    let world = seeded().await;
    let store = world.store.clone();
    let action = stored_proposal(world.incident.id());
    let id = action.id();
    ActionRepo::insert(&*store, &action).await.unwrap();
    let mut a = world.start(
        ActionSettings::default(),
        Arc::new(RefusingAuditUow::new(store)),
    );

    let refused = a.service.approve(id, human(UserId::new()), None).await;
    assert!(
        matches!(refused, Err(ApprovalError::Store(_))),
        "an audit failure fails the transition: {refused:?}",
    );

    let stored = a.get(id).await;
    assert_eq!(stored.status(), &ActionStatus::Proposed, "nothing changed");
    assert_eq!(
        stored.chat_token_hash(),
        Some(&[0x11; 32]),
        "the chat token the action already had is still usable",
    );
    assert!(
        a.trail(id).await.is_empty(),
        "no audit entry was written for the refused transition",
    );
    assert!(
        journal_rows(&a.store, id).await.is_empty(),
        "no journal entry was written for the refused transition",
    );
    a.settle().await;
    assert!(
        !a.drain()
            .iter()
            .any(|event| matches!(event.kind, EventKind::ActionApproved { .. })),
        "nothing was published",
    );
}

#[tokio::test]
async fn every_action_transition_commits_state_audit_and_journal_together() {
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;

    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    a.service
        .approve(action.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.until(action.id(), |status| status == &ActionStatus::Verified)
        .await;

    let commits = probe.commits();
    assert_eq!(commits.len(), 5, "one commit per transition, nothing else");
    let expected = [
        (None, "proposed"),
        (Some("proposed"), "approved"),
        (Some("approved"), "dispatched"),
        (Some("dispatched"), "succeeded"),
        (Some("succeeded"), "verified"),
    ];
    for (tx, (from, to)) in commits.iter().zip(expected) {
        assert_transition(tx, from, to);
    }
    assert_eq!(
        commits
            .iter()
            .map(|tx| tx.audit[0].kind.clone())
            .collect::<Vec<_>>(),
        [
            audit::PROPOSED,
            audit::APPROVED,
            audit::DISPATCHED,
            audit::SUCCEEDED,
            audit::VERIFIED,
        ]
    );
}

#[tokio::test]
async fn denied_and_expired_and_rejected_and_blocked_and_failed_transitions_are_atomic() {
    // A denial.
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    a.service
        .deny(action.id(), human(UserId::new()))
        .await
        .unwrap();
    let commits = probe.commits();
    assert_eq!(commits.len(), 2);
    assert_transition(&commits[1], Some("proposed"), "denied");
    assert_eq!(commits[1].audit[0].kind, audit::DENIED);

    // The expiry sweep.
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    a.propose_via_service(
        &verified_restart(),
        &[("container", "nginx")],
        SignedDuration::from_mins(10),
    )
    .await
    .unwrap();
    a.clock.advance(SignedDuration::from_mins(11));
    assert_eq!(a.service.expire_due().await.unwrap(), 1);
    let commits = probe.commits();
    assert_eq!(commits.len(), 2);
    assert_transition(&commits[1], Some("proposed"), "expired");
    assert_eq!(commits[1].audit[0].kind, audit::EXPIRED);

    // A plan mismatch at dispatch.
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    let mut changed = verified_restart();
    changed.manifest_hash = "cd".repeat(32).parse().unwrap();
    a.link.set_skills(host_id(), vec![changed]);
    a.service
        .approve(action.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Rejected { .. })
    })
    .await;
    let commits = probe.commits();
    let last = commits.last().unwrap();
    assert_transition(last, Some("approved"), "rejected");
    assert_eq!(last.audit[0].kind, audit::REJECTED);

    // The loop guard's block.
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    for _ in 0..3 {
        let (action, _) = a
            .propose_via_service(
                &verified_restart(),
                &[("container", "nginx")],
                SignedDuration::from_hours(1),
            )
            .await
            .unwrap();
        a.service
            .approve(action.id(), human(UserId::new()), None)
            .await
            .unwrap();
        a.until(action.id(), |status| status == &ActionStatus::Verified)
            .await;
    }
    let (fourth, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    a.service
        .approve(fourth.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.until(fourth.id(), |status| {
        matches!(status, ActionStatus::Rejected { .. })
    })
    .await;
    let commits = probe.commits();
    let last = commits.last().unwrap();
    assert_transition(last, Some("approved"), "rejected");
    assert_eq!(last.audit[0].kind, audit::BLOCKED);

    // A run that fails.
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    a.link.on_skill(|_, _| {
        Ok(SkillOutput {
            exit_code: Some(3),
            ..ok_output("boom")
        })
    });
    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    a.service
        .approve(action.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Failed { .. })
    })
    .await;
    let commits = probe.commits();
    let last = commits.last().unwrap();
    assert_transition(last, Some("dispatched"), "failed");
    assert_eq!(last.audit[0].kind, audit::FAILED);

    // A re-check that still fails.
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    a.link.on_check(|_, _, _| Ok(check(SignalStatus::Firing)));
    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    a.service
        .approve(action.id(), human(UserId::new()), None)
        .await
        .unwrap();
    a.until(action.id(), |status| status == &ActionStatus::VerifyFailed)
        .await;
    let commits = probe.commits();
    let last = commits.last().unwrap();
    assert_transition(last, Some("succeeded"), "verify_failed");
    assert_eq!(last.audit[0].kind, audit::VERIFY_FAILED);

    // A run the previous hub session left dispatched.
    let world = seeded().await;
    let skill = verified_restart();
    let params: BTreeMap<String, String> = [("container", "nginx")]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    let validated = skill.validate_args(&params).unwrap();
    let mut action = ActionRequest::propose(
        world.incident.id(),
        host_id(),
        &skill,
        &validated,
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    );
    action.approve(human(UserId::new()), T0).unwrap();
    let plan = PlanHash::compute(&skill.name, skill.manifest_hash, action.host(), &validated);
    action.dispatch(plan, T0).unwrap();
    ActionRepo::insert(&*world.store, &action).await.unwrap();
    let probe = Arc::new(RecordingUow::new(world.store.clone()));
    let a = world.start(ActionSettings::default(), probe.clone());
    a.settle().await;
    let commits = probe.commits();
    assert_eq!(commits.len(), 1, "the restart fails the leftover once");
    assert_transition(&commits[0], Some("dispatched"), "failed");
    assert_eq!(commits[0].audit[0].kind, audit::FAILED);
}

#[tokio::test]
async fn stale_decision_is_refused_and_writes_nothing() {
    let world = seeded().await;
    let action = stored_proposal(world.incident.id());
    let mut rival = action.clone();
    rival.deny(T0).unwrap();
    ActionRepo::insert(&*world.store, &action).await.unwrap();
    let mut a = world.start(
        ActionSettings::default(),
        Arc::new(StaleRaceUow::new(world.store.clone(), rival)),
    );

    let refused = a
        .service
        .approve(action.id(), human(UserId::new()), None)
        .await;
    assert_eq!(
        refused.err(),
        Some(ApprovalError::Refused(ActionError::ChangedMeanwhile)),
        "a decision read before the action changed is refused",
    );
    assert_eq!(
        a.get(action.id()).await.status(),
        &ActionStatus::Denied,
        "the rival decision is what stays stored",
    );
    assert!(
        a.trail(action.id()).await.is_empty(),
        "the refused decision wrote no audit entry",
    );
    assert!(
        journal_rows(&a.store, action.id()).await.is_empty(),
        "the refused decision wrote no journal entry",
    );
    a.settle().await;
    assert!(a.drain().is_empty(), "nothing was published");
}

#[tokio::test]
async fn chat_approval_audit_actor_is_the_user_with_the_channel_in_the_detail() {
    let a = actions(ActionSettings::default()).await;
    let (action, token) = a.propose().await;
    let user = UserId::new();
    a.service
        .approve(action.id(), chat(user, "discord", "42"), Some(&token))
        .await
        .unwrap();

    let entry = AuditLog::list(
        &*a.store,
        &AuditFilter {
            subject: Some(format!("action:{}", action.id())),
            kind: Some(audit::APPROVED.to_owned()),
            ..AuditFilter::default()
        },
        &Page::first(1),
    )
    .await
    .unwrap()
    .items
    .remove(0);
    assert_eq!(entry.actor, format!("user:{user}"));
    assert_eq!(entry.detail["channel"]["platform_user"], "42");
    assert_eq!(entry.detail["channel"]["type"], "chat");
}

#[tokio::test]
async fn proposed_action_is_stored_with_its_audit_and_journal_rows() {
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    let (action, _) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();

    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);
    assert_eq!(a.trail(action.id()).await, [audit::PROPOSED.to_owned()]);
    assert_eq!(
        journal_rows(&a.store, action.id()).await,
        [JournalKind::ActionTransition.as_str().to_owned()]
    );
    let commits = probe.commits();
    assert_eq!(commits.len(), 1);
    assert_transition(&commits[0], None, "proposed");
}

#[tokio::test]
async fn propose_rejects_an_action_that_is_not_proposed() {
    let a = actions(ActionSettings::default()).await;
    let (mut action, _) = a
        .proposal(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await;
    action.approve(human(UserId::new()), a.now()).unwrap();

    let refused = a.service.propose(action.clone()).await;
    assert!(
        matches!(
            refused,
            Err(ApprovalError::Refused(ActionError::NotAllowed {
                operation: "propose",
                ..
            }))
        ),
        "a proposal can only store a proposed action: {refused:?}",
    );
    assert!(
        ActionRepo::get(&*a.store, action.id())
            .await
            .unwrap()
            .is_none(),
        "nothing was stored",
    );
    assert!(a.trail(action.id()).await.is_empty());
}

#[tokio::test]
async fn token_rotation_goes_through_the_writer() {
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    let (action, old_token) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();

    let new_token = mint_token();
    let new_hash = token_hash(&new_token);
    let rotated = a
        .service
        .rotate_chat_token(action.id(), new_hash)
        .await
        .unwrap();
    assert_eq!(
        rotated.chat_token_hash(),
        Some(&new_hash),
        "the action the writer answered carries the new hash",
    );
    assert_eq!(
        a.get(action.id()).await.chat_token_hash(),
        Some(&new_hash),
        "the new hash is stored",
    );
    assert_eq!(
        a.service
            .approve(
                action.id(),
                chat(UserId::new(), "discord", "1"),
                Some(&old_token)
            )
            .await
            .err(),
        Some(ApprovalError::BadToken),
        "the old token stops working",
    );

    let commits = probe.commits();
    let last = commits.last().unwrap();
    assert_eq!(last.writes.len(), 1);
    assert_eq!(last.audit[0].kind, audit::TOKEN_ROTATED);
    assert_eq!(last.journal[0].payload["change"], "chat_token_rotated");
    assert_transition(last, Some("proposed"), "proposed");
}

#[tokio::test]
async fn token_rotation_records_no_token_material() {
    let store = Arc::new(InMemoryStore::new());
    let probe = Arc::new(RecordingUow::new(store.clone()));
    let a = actions_with_uow(store, probe.clone(), ActionSettings::default()).await;
    let (action, old_token) = a
        .propose_via_service(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await
        .unwrap();
    let new_token = mint_token();
    a.service
        .rotate_chat_token(action.id(), token_hash(&new_token))
        .await
        .unwrap();

    let audit_row = AuditLog::list(
        &*a.store,
        &AuditFilter {
            subject: Some(format!("action:{}", action.id())),
            kind: Some(audit::TOKEN_ROTATED.to_owned()),
            ..AuditFilter::default()
        },
        &Page::first(1),
    )
    .await
    .unwrap()
    .items
    .remove(0);
    let journal = JournalRead::entries_for(&*a.store, &AggregateRef::action(action.id()), 10)
        .await
        .unwrap();
    let last = journal.last().unwrap();
    assert_eq!(last.payload["change"], "chat_token_rotated");
    let recorded = format!("{audit_row:?} {last:?}");
    for secret in [
        old_token.clone(),
        new_token.clone(),
        hex::encode(token_hash(&old_token)),
        hex::encode(token_hash(&new_token)),
    ] {
        assert!(
            !recorded.contains(&secret),
            "token material reached the audit log or journal: {secret}",
        );
    }
}

#[tokio::test]
async fn rotation_of_a_decided_action_is_refused() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    a.service
        .deny(action.id(), human(UserId::new()))
        .await
        .unwrap();

    let refused = a
        .service
        .rotate_chat_token(action.id(), token_hash(&mint_token()))
        .await;
    assert_eq!(
        refused.err(),
        Some(ApprovalError::Refused(ActionError::ChangedMeanwhile)),
    );
    let stored = a.get(action.id()).await;
    assert_eq!(stored.status(), &ActionStatus::Denied);
    assert_eq!(
        stored.chat_token_hash(),
        None,
        "a decided action has no chat token to rotate",
    );
    assert!(
        a.trail(action.id())
            .await
            .iter()
            .all(|kind| kind != audit::TOKEN_ROTATED),
    );
}
