//! The phases of one block: ordering, the confirmation window, the failure paths.

use domovoi_core::StepOutcome;
use domovoi_core::attest::{Authority, ExpectedSafety};
use domovoi_core::containment::{ContainCall, keys};
use domovoi_core::ports::{BlockPhase, Presence, StateWrite};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

use super::world::{World, ok, rejected};
use crate::containment::phases::{ContainEnd, execute_containment};

const TTL: i64 = 600;

async fn run(
    world: &World,
    source: &str,
    authority: Authority,
) -> (ContainEnd, domovoi_core::ExecutionId) {
    let (authorized, prepared, session, epoch) = world.step(source, TTL, authority).await;
    let id = authorized.execution().execution;
    let end =
        execute_containment(world.driver.deps(), &authorized, &prepared, session, epoch).await;
    (end, id)
}

fn happy(world: &World) {
    world
        .relay
        .queue_result(ok(&[(keys::OUT_EXISTING_FLOWS, "n/a")]));
    world
        .relay
        .queue_result(ok(&[(keys::OUT_BRIDGE_GAP, "false")]));
}

#[tokio::test]
async fn apply_is_committed_applying_before_the_request_is_sent() {
    let world = World::new().await;
    world.verify();
    happy(&world);
    run(&world, "198.51.100.0/24", Authority::Human).await;
    let seen = world.relay.seen_phases();
    assert_eq!(
        seen[0],
        ("dispatch:apply".to_owned(), Some(BlockPhase::Applying))
    );
    // The confirmation too is committed before it is sent.
    assert_eq!(
        seen[1],
        ("dispatch:confirm".to_owned(), Some(BlockPhase::Confirming))
    );
}

#[tokio::test]
async fn confirm_is_sent_within_20s_of_the_apply_result() {
    let world = World::new().await;
    world.verify();
    happy(&world);
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::Succeeded,
            ..
        }
    ));
    assert_eq!(
        world.relay.events(),
        ["dispatch:apply", "result", "dispatch:confirm", "result"]
    );
    let row = world.block(id).await;
    assert_eq!(row.phase, BlockPhase::Confirmed);
    assert_eq!(
        row.deadline,
        T0.checked_add(SignedDuration::from_secs(TTL)).ok()
    );
}

/// A commit of the `Applied` row that takes 25 seconds: the confirmation misses its window.
fn spend_window_on_applied(world: &World, then_drop_session: bool) {
    let clock = world.clock.clone();
    let link = world.link.clone();
    let host = world.host;
    world.uow.on_commit(move |tx| {
        let applied = tx.writes.iter().any(|write| {
            matches!(
                write,
                StateWrite::Containment(domovoi_core::ports::ContainmentWrite::Phase { update, .. })
                    if update.phase == BlockPhase::Applied
            )
        });
        if applied {
            clock.advance(SignedDuration::from_secs(25));
            if then_drop_session {
                link.set_presence(host, Presence::Disconnected { last_seen: None });
            }
        }
    });
}

#[tokio::test]
async fn confirm_not_sent_in_time_rolls_back_when_the_session_lives() {
    let world = World::new().await;
    world.verify();
    world.relay.queue_result(ok(&[]));
    world.relay.queue_result(ok(&[]));
    spend_window_on_applied(&world, false);
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::Failed,
            ..
        }
    ));
    assert_eq!(
        world.relay.events(),
        ["dispatch:apply", "result", "dispatch:rollback", "result"]
    );
    assert_eq!(world.block(id).await.phase, BlockPhase::RolledBack);
}

#[tokio::test]
async fn confirm_not_sent_in_time_expires_when_the_session_is_gone() {
    let world = World::new().await;
    world.verify();
    world.relay.queue_result(ok(&[]));
    spend_window_on_applied(&world, true);
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::Failed,
            ..
        }
    ));
    // Nothing more was sent: the kernel removes the block by itself.
    assert_eq!(world.relay.events(), ["dispatch:apply", "result"]);
    assert_eq!(world.block(id).await.phase, BlockPhase::Expired);
}

#[tokio::test]
async fn expired_confirm_does_not_reapply() {
    let world = World::new().await;
    world.verify();
    world.relay.queue_result(ok(&[]));
    world
        .relay
        .queue_result(rejected("expired: the element was gone"));
    let (_end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert_eq!(world.block(id).await.phase, BlockPhase::Expired);
    let events = world.relay.events();
    assert_eq!(events.iter().filter(|e| *e == "dispatch:apply").count(), 1);
    assert_eq!(
        events.iter().filter(|e| *e == "dispatch:rollback").count(),
        0
    );
}

#[tokio::test]
async fn containment_lost_settles_lost_and_never_reapplies() {
    let world = World::new().await;
    world.verify();
    world.relay.queue_result(ok(&[]));
    world
        .relay
        .queue_result(rejected("containment_lost: element_missing"));
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::Failed,
            ..
        }
    ));
    assert_eq!(world.block(id).await.phase, BlockPhase::Lost);
    assert_eq!(
        world.relay.events(),
        ["dispatch:apply", "result", "dispatch:confirm", "result"]
    );
}

#[tokio::test]
async fn phases_share_one_plan_hash_and_distinct_execution_ids() {
    let world = World::new().await;
    world.verify();
    happy(&world);
    let (_end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    world.relay.queue_result(ok(&[]));
    let released = world.driver.release(id).await.unwrap();
    assert_eq!(released.phase, BlockPhase::RolledBack);
    let sent = world.relay.dispatches();
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|d| d.plan_hash() == sent[0].plan_hash()));
    let mut ids: Vec<_> = sent.iter().map(|d| d.execution()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "each phase has its own execution id");
    // The confirmation and the rollback name the block by the apply's execution id.
    for dispatch in &sent[1..] {
        let spec = dispatch.call().spec().unwrap();
        assert_eq!(spec.block, id);
        assert_eq!(
            dispatch.call().args()[keys::ARG_BLOCK],
            id.as_uuid().to_string()
        );
    }
    assert!(matches!(sent[0].call(), ContainCall::Apply(_)));
    assert!(matches!(sent[1].call(), ContainCall::Confirm(_)));
    assert!(matches!(sent[2].call(), ContainCall::Rollback(_)));
}

#[tokio::test]
async fn verified_dispatch_carries_the_stored_identity() {
    let world = World::new().await;
    world.verify();
    happy(&world);
    let (_end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    let row = world.block(id).await;
    let stored: serde_json::Value = serde_json::from_str(&row.assurance).unwrap();
    assert_eq!(stored["kind"], "verified");
    for dispatch in world.relay.dispatches() {
        let Some(ExpectedSafety::Verified { generation, .. }) = dispatch.expected_safety() else {
            panic!("a verified block sends a verified identity at every phase");
        };
        assert_eq!(*generation, 1);
    }
}

#[tokio::test]
async fn unverified_human_block_sends_the_reason_and_audits_it() {
    let world = World::new().await;
    // No key is pinned for the helper.
    happy(&world);
    let (_end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    for dispatch in world.relay.dispatches() {
        assert!(matches!(
            dispatch.expected_safety(),
            Some(ExpectedSafety::Unverified { .. })
        ));
    }
    let row = world.block(id).await;
    assert_eq!(
        super::super::audit::assurance_kind(&row.assurance),
        "unverified"
    );
    let audits = world.audits().await;
    assert!(
        audits
            .iter()
            .any(|a| a.kind == "containment.unverified_approved")
    );
    assert!(
        audits
            .iter()
            .all(|a| !a.detail.to_string().contains("password"))
    );
}

#[tokio::test]
async fn a_refused_apply_leaves_not_run_with_its_token() {
    let world = World::new().await;
    world.verify();
    world
        .relay
        .queue_result(rejected("exclusion_overlap: 192.0.2.0/24"));
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::Refused,
            ..
        }
    ));
    let row = world.block(id).await;
    assert_eq!(row.phase, BlockPhase::NotRun);
    assert_eq!(row.failure_token.as_deref(), Some("exclusion_overlap"));
    assert_eq!(world.relay.events(), ["dispatch:apply", "result"]);
}

#[tokio::test]
async fn a_failed_hand_off_after_the_applying_commit_leaves_not_run() {
    let world = World::new().await;
    world.verify();
    world
        .relay
        .queue_dispatch(Some(domovoi_core::ports::LinkError::NotConnected));
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert!(matches!(
        end,
        ContainEnd::Settled {
            outcome: StepOutcome::NotRun,
            ..
        }
    ));
    assert_eq!(world.block(id).await.phase, BlockPhase::NotRun);
    assert!(world.relay.dispatches().is_empty());
}

#[tokio::test]
async fn a_timed_out_apply_stays_applying_for_reconcile() {
    let world = World::new().await;
    world.verify();
    // No answer scripted: the result times out after the hand-off.
    let (end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    assert_eq!(end, ContainEnd::Unknown);
    assert_eq!(world.block(id).await.phase, BlockPhase::Applying);
    let audits = world.audits().await;
    assert!(
        audits
            .iter()
            .any(|a| a.kind == "containment.outcome_unknown")
    );
}

#[tokio::test]
async fn audit_rows_exist_for_every_phase_and_hold_no_secret() {
    let world = World::new().await;
    world.verify();
    happy(&world);
    let (_end, id) = run(&world, "198.51.100.0/24", Authority::Human).await;
    world.relay.queue_result(ok(&[]));
    world.driver.release(id).await.unwrap();
    let kinds: Vec<String> = world.audits().await.into_iter().map(|a| a.kind).collect();
    for expected in [
        "containment.requested",
        "containment.applying",
        "containment.applied",
        "containment.confirming",
        "containment.confirmed",
        "containment.released",
        "containment.rolled_back",
    ] {
        assert!(
            kinds.iter().any(|k| k == expected),
            "missing {expected}: {kinds:?}"
        );
    }
    for row in world.audits().await {
        let text = row.detail.to_string();
        assert!(!text.contains("password") && !text.contains("signature"));
        assert_eq!(row.subject, format!("host:{}", world.host));
    }
}
