use domovoi_core::ports::{AuditFilter, AuditLog, IncidentRepo, Page, Presence, SkillOutput};
use domovoi_core::{ActionStatus, ManifestHash, Risk, SignalStatus, UserId, Verdict};
use domovoi_engine::notify::{NotifyAction, NotifySettings, plan};
use domovoi_engine::{ActionSettings, EventKind};
use domovoi_testkit::approvals::human;
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;
use jiff::tz::TimeZone;

use crate::actions_harness::{Actions, actions, check, ok_output, verified_restart, wipe};
use crate::harness::{SECRET, host_id, restart_skill};

async fn approve(a: &Actions, id: domovoi_core::ActionId) {
    a.service
        .approve(id, human(UserId::new()), None)
        .await
        .unwrap();
}

fn posted() -> domovoi_core::NotifyState {
    let mut state = domovoi_core::NotifyState::default();
    state.messages.insert(
        "discord".to_owned(),
        domovoi_core::ChatRefs {
            message: domovoi_core::ports::MessageRef {
                platform: "discord".to_owned(),
                id: "m1".to_owned(),
            },
            thread: None,
        },
    );
    state
}

#[tokio::test]
async fn approved_write_action_dispatches_succeeds_and_verifies() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    approve(&a, action.id()).await;
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Verified))
        .await;
    assert_eq!(done.status(), &ActionStatus::Verified);

    let requests = a.link.requests();
    let dispatch = requests
        .iter()
        .find_map(|r| match r {
            LinkRequest::Skill { host, dispatch, .. } => Some((host, dispatch)),
            _ => None,
        })
        .expect("dispatched");
    assert_eq!(*dispatch.0, host_id());
    assert_eq!(dispatch.1.action, action.id());
    assert_eq!(dispatch.1.plan_hash, action.plan_hash());
    assert_eq!(dispatch.1.args.get("container"), Some("nginx"));
    assert!(requests.iter().any(|r| matches!(
        r,
        LinkRequest::Check { check, .. } if check.as_str() == "docker.container"
    )));

    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(incident.verdict(), Some(Verdict::Fixed));
}

#[tokio::test]
async fn failed_verification_reaches_a_human() {
    let mut a = actions(ActionSettings::default()).await;
    a.link.on_check(|_, _, _| Ok(check(SignalStatus::Firing)));
    let (action, _) = a.propose().await;
    approve(&a, action.id()).await;
    a.until(action.id(), |s| matches!(s, ActionStatus::VerifyFailed))
        .await;
    let event = a
        .drain()
        .into_iter()
        .find(|e| {
            matches!(
                &e.kind,
                EventKind::ActionChanged {
                    status: ActionStatus::VerifyFailed,
                    ..
                }
            )
        })
        .expect("the change is published");
    let actions = plan(
        &event,
        &posted(),
        &NotifySettings::new(TimeZone::UTC),
        a.now(),
    )
    .actions;
    assert!(matches!(
        &actions[..],
        [NotifyAction::ThreadReply { text, .. }, NotifyAction::UpdateCard { .. }]
            if text.contains("check still fails")
    ));
    let incident = IncidentRepo::get(&*a.store, a.incident.id())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(incident.verdict(), Some(Verdict::Fixed));
}

#[tokio::test]
async fn manifest_change_between_approval_and_dispatch_rejects() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let mut changed = verified_restart();
    changed.manifest_hash = "cd".repeat(32).parse::<ManifestHash>().unwrap();
    a.link.set_skills(host_id(), vec![changed]);
    approve(&a, action.id()).await;
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    assert_eq!(
        done.status(),
        &ActionStatus::Rejected {
            reason: "the plan changed after approval".to_owned()
        }
    );

    let (action, _) = a.propose().await;
    let mut riskier = verified_restart();
    riskier.risk = Risk::Destructive;
    a.link.set_skills(host_id(), vec![riskier]);
    approve(&a, action.id()).await;
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    assert!(matches!(done.status(), ActionStatus::Rejected { reason } if reason.contains("risk")));

    a.link.set_skills(host_id(), vec![wipe()]);
    let (action, _) = a
        .propose_for(
            &verified_restart(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await;
    approve(&a, action.id()).await;
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    assert!(
        matches!(done.status(), ActionStatus::Rejected { reason } if reason.contains("no longer on the host"))
    );
    assert_eq!(a.runs(), 0);
}

#[tokio::test]
async fn args_no_longer_valid_reject() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let mut stricter = verified_restart();
    stricter.params[0].kind = domovoi_core::ParamKind::Enum {
        values: vec!["plex".to_owned()],
    };
    a.link.set_skills(host_id(), vec![stricter]);
    approve(&a, action.id()).await;
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    assert!(matches!(
        done.status(),
        ActionStatus::Rejected { reason } if reason.contains("no longer valid")
    ));
    assert_eq!(a.runs(), 0);
    assert_eq!(
        a.trail(action.id()).await,
        ["action.proposed", "action.approved", "action.rejected"]
    );
}

#[tokio::test]
async fn loop_guard_blocks_fourth_dispatch_in_an_hour() {
    let mut a = actions(ActionSettings::default()).await;
    a.link.set_skills(host_id(), vec![restart_skill()]);
    for _ in 0..3 {
        let (action, _) = a
            .propose_for(
                &restart_skill(),
                &[("container", "nginx")],
                SignedDuration::from_hours(1),
            )
            .await;
        approve(&a, action.id()).await;
        a.until(action.id(), |s| matches!(s, ActionStatus::Succeeded))
            .await;
    }
    let (fourth, _) = a
        .propose_for(
            &restart_skill(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await;
    a.drain();
    approve(&a, fourth.id()).await;
    let done = a
        .until(fourth.id(), |s| matches!(s, ActionStatus::Rejected { .. }))
        .await;
    assert!(
        matches!(done.status(), ActionStatus::Rejected { reason } if reason.starts_with("loop guard"))
    );
    assert_eq!(a.runs(), 3);
    let blocked = a
        .drain()
        .into_iter()
        .find(|e| matches!(e.kind, EventKind::ActionBlocked { .. }))
        .expect("the block reaches a human");
    let planned = plan(
        &blocked,
        &posted(),
        &NotifySettings::new(TimeZone::UTC),
        a.now(),
    )
    .actions;
    assert!(matches!(&planned[0], NotifyAction::ThreadReply { .. }));
    assert_eq!(
        a.trail(fourth.id()).await,
        ["action.proposed", "action.approved", "action.blocked"]
    );

    // An hour later the guard lets it through again.
    a.clock.advance(SignedDuration::from_mins(61));
    let (fifth, _) = a
        .propose_for(
            &restart_skill(),
            &[("container", "nginx")],
            SignedDuration::from_hours(1),
        )
        .await;
    approve(&a, fifth.id()).await;
    a.until(fifth.id(), |s| matches!(s, ActionStatus::Succeeded))
        .await;
}

#[tokio::test]
async fn offline_agent_waits_then_fails() {
    let a = actions(ActionSettings::default()).await;
    a.link
        .set_presence(host_id(), Presence::Disconnected { last_seen: None });
    let (action, _) = a.propose().await;
    let start = a.now();
    approve(&a, action.id()).await;
    let done = a
        .until(action.id(), |s| matches!(s, ActionStatus::Failed { .. }))
        .await;
    assert_eq!(
        done.status(),
        &ActionStatus::Failed {
            reason: "agent offline".to_owned()
        }
    );
    assert!(a.now().duration_since(start) >= SignedDuration::from_mins(5));
    assert_eq!(a.runs(), 0);

    // An agent that comes back in time runs the action.
    let (action, _) = a.propose().await;
    approve(&a, action.id()).await;
    for _ in 0..3 {
        a.settle().await;
        a.clock.advance(SignedDuration::from_secs(5));
    }
    a.link.set_presence(
        host_id(),
        Presence::Connected {
            since: a.now(),
            version: "0.1.0".to_owned(),
        },
    );
    a.until(action.id(), |s| matches!(s, ActionStatus::Verified))
        .await;
}

#[tokio::test]
async fn audit_trail_is_complete_and_ordered() {
    let a = actions(ActionSettings::default()).await;
    let (ok, _) = a.propose().await;
    approve(&a, ok.id()).await;
    a.until(ok.id(), |s| matches!(s, ActionStatus::Verified))
        .await;
    assert_eq!(
        a.trail(ok.id()).await,
        [
            "action.proposed",
            "action.approved",
            "action.dispatched",
            "action.succeeded",
            "action.verified"
        ]
    );

    a.link.on_skill(|_, _| {
        Ok(SkillOutput {
            exit_code: Some(3),
            ..ok_output("boom")
        })
    });
    let (failed, _) = a.propose().await;
    approve(&a, failed.id()).await;
    a.until(failed.id(), |s| matches!(s, ActionStatus::Failed { .. }))
        .await;
    assert_eq!(
        a.trail(failed.id()).await,
        [
            "action.proposed",
            "action.approved",
            "action.dispatched",
            "action.failed"
        ]
    );

    a.link.on_skill(|_, _| Ok(ok_output("done")));
    a.link.on_check(|_, _, _| Ok(check(SignalStatus::Firing)));
    let (unverified, _) = a.propose().await;
    approve(&a, unverified.id()).await;
    a.until(unverified.id(), |s| matches!(s, ActionStatus::VerifyFailed))
        .await;
    assert_eq!(
        a.trail(unverified.id()).await,
        [
            "action.proposed",
            "action.approved",
            "action.dispatched",
            "action.succeeded",
            "action.verify_failed"
        ]
    );

    let filter = AuditFilter {
        subject: Some(format!("action:{}", ok.id())),
        kind: Some("action.succeeded".to_owned()),
        ..AuditFilter::default()
    };
    let entry = AuditLog::list(&*a.store, &filter, &Page::first(1))
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(entry.actor, "system:actions");
    assert_eq!(entry.detail["plan_hash"], ok.plan_hash().to_string());
    assert_eq!(entry.detail["argv_hash"], "ab".repeat(32));
    assert_eq!(entry.detail["exit_code"], 0);
    assert_eq!(entry.detail["duration_ms"], 1200);
    let approved = AuditLog::list(
        &*a.store,
        &AuditFilter {
            subject: Some(format!("action:{}", ok.id())),
            kind: Some("action.approved".to_owned()),
            ..AuditFilter::default()
        },
        &Page::first(1),
    )
    .await
    .unwrap()
    .items
    .remove(0);
    assert!(approved.actor.starts_with("user:"));
}

#[tokio::test]
async fn output_excerpt_is_redacted_and_capped() {
    let a = actions(ActionSettings::default()).await;
    a.link.on_skill(|_, _| {
        Ok(SkillOutput {
            stderr: format!("warning: token={SECRET}"),
            ..ok_output(&format!("using {SECRET}\n{}", "line\n".repeat(4000)))
        })
    });
    let (action, _) = a.propose().await;
    approve(&a, action.id()).await;
    a.until(action.id(), |s| matches!(s, ActionStatus::Verified))
        .await;
    let filter = AuditFilter {
        kind: Some("action.succeeded".to_owned()),
        ..AuditFilter::default()
    };
    let entry = AuditLog::list(&*a.store, &filter, &Page::first(1))
        .await
        .unwrap()
        .items
        .remove(0);
    let output = entry.detail["output"].as_str().unwrap();
    assert!(output.len() <= 4096, "{}", output.len());
    assert!(output.starts_with("using [REDACTED]"));
    assert!(!entry.detail.to_string().contains(SECRET));
    assert_eq!(entry.detail["output_sha256"].as_str().unwrap().len(), 64);
}
