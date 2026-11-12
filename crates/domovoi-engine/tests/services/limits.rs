use domovoi_core::ports::LlmError;
use domovoi_core::ports::{AdvisoryRepo, IncidentRepo, IncidentTimelineEntry, UsageRepo};
use domovoi_core::{ActionStatus, Incident, IncidentId, TriageOutcome, UserId};
use domovoi_engine::EventKind;
use domovoi_engine::triage::{FAILED_RETRY_LIMIT, TriageError, TriageSettings, token_hash};
use domovoi_testkit::build::{open_incident, resolved_incident};
use jiff::SignedDuration;
use serde_json::json;
use tokio::task::yield_now;

use crate::harness::{
    Setup, World, add_signal, disk, host_id, report, report_json, restart_skill, world,
};

/// Gives the service's tasks turns to run their own awaits between clock jumps.
async fn settle() {
    for _ in 0..500 {
        yield_now().await;
    }
}

/// Counts the incident's timeline entries of a kind.
async fn count(w: &World, id: IncidentId, kind: &str) -> usize {
    IncidentRepo::timeline(&*w.store, id)
        .await
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .count()
}

async fn total_skips(w: &World, others: &[Incident]) -> usize {
    let mut total = 0;
    for other in others {
        total += count(w, other.id(), "triage.skipped").await;
    }
    total
}

#[tokio::test]
async fn per_hour_cap_and_monthly_budget_skip_without_model_call() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        monthly_budget_micros: Some(1_000_000),
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert!(matches!(
        w.run().await,
        TriageOutcome::Skipped {
            reason,
        } if reason.contains("per hour")
    ));
    assert_eq!(w.primary.requests().len(), 1);

    w.clock.advance(SignedDuration::from_mins(61));
    UsageRepo::record(&*w.store, "anthropic", 10, 10, 1_000_000, w.now())
        .await
        .unwrap();
    assert!(matches!(
        w.run().await,
        TriageOutcome::Skipped { reason } if reason.contains("monthly")
    ));
    assert_eq!(
        w.primary.requests().len(),
        1,
        "no model call when over a cap"
    );
}

#[tokio::test]
async fn hourly_cap_skip_runs_again_when_a_slot_frees() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert!(matches!(
        w.run().await,
        TriageOutcome::Skipped {
            reason,
        } if reason.contains("per hour") && reason.contains("tries again from")
    ));

    // The retry runs through the queue, so give the service's tasks turns to reach it.
    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    assert_eq!(
        w.primary.requests().len(),
        2,
        "the skipped triage runs when a slot frees",
    );
    let timeline = IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    let last = timeline
        .iter()
        .rfind(|e| e.kind.starts_with("triage.") && e.kind != "triage.followup")
        .unwrap()
        .kind
        .clone();
    assert_eq!(
        last, "triage.report",
        "the retry ends the timeline with a report"
    );
}

#[tokio::test]
async fn failed_triage_runs_again_after_the_retry_delay() {
    let settings = TriageSettings {
        per_hour_runs: 10,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    assert_eq!(w.primary.requests().len(), 1);

    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    assert_eq!(
        w.primary.requests().len(),
        2,
        "a failed triage runs again after the retry delay",
    );
    let timeline = IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    let last = timeline
        .iter()
        .rfind(|e| e.kind.starts_with("triage.") && e.kind != "triage.followup")
        .unwrap()
        .kind
        .clone();
    assert_eq!(last, "triage.report", "the retry produced the report");
}

#[tokio::test]
async fn repeat_incident_uses_cache_without_model_call() {
    let mut w = world(Setup::default()).await;
    w.primary.push_text(report());
    let TriageOutcome::Report(first) = w.run().await else {
        panic!("the first run calls the model");
    };

    // The same check on the same target resolved earlier with the same evidence.
    let earlier = resolved_incident(host_id(), "docker.container", Some("nginx"), w.now()).unwrap();
    IncidentRepo::insert(&*w.store, &earlier).await.unwrap();
    w.signal(disk(92.1)).await;
    w.drain();
    let outcome = w.run().await;
    assert_eq!(
        outcome,
        TriageOutcome::Cached {
            report: first,
            from: earlier.id()
        }
    );
    assert_eq!(w.primary.requests().len(), 1, "no second model call");
    assert!(w.drain().iter().any(|e| matches!(
        &e.kind,
        EventKind::TriageCompleted {
            outcome: TriageOutcome::Cached { .. },
            ..
        }
    )));

    // Different evidence is a different situation.
    w.signal(disk(55.0)).await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert_eq!(w.primary.requests().len(), 2);
}

#[tokio::test]
async fn a_cached_report_stores_its_proposals_as_actions() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let answer = report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    );
    w.primary.push_text(answer);
    let TriageOutcome::Report(first) = w.run().await else {
        panic!("the first run calls the model");
    };
    assert_eq!(first.proposals.len(), 1, "the fresh run kept the proposal");
    w.drain();

    // The first incident closes, then the same problem files as a new open incident
    // after the reopen window; the evidence buckets the same as the cached run's.
    w.manager
        .dismiss(
            w.incident.id(),
            UserId::new(),
            "closed on the test".to_owned(),
        )
        .await
        .unwrap();
    w.clock.advance(SignedDuration::from_hours(7));
    settle().await;
    let again = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*w.store, &again).await.unwrap();
    add_signal(&w.store, &again, disk(92.1), w.now()).await;

    assert_eq!(
        w.triage.triage(&again).await,
        TriageOutcome::Cached {
            report: first,
            from: w.incident.id()
        },
    );
    assert_eq!(
        w.primary.requests().len(),
        1,
        "the repeat never calls the model"
    );

    let tokens = w.tokens();
    assert_eq!(tokens.len(), 1, "the cached report minted one chat token");
    let stored = w
        .actions()
        .await
        .into_iter()
        .filter(|a| a.incident() == again.id())
        .collect::<Vec<_>>();
    assert_eq!(
        stored.len(),
        1,
        "the cached report stored one action for the repeat"
    );
    assert_eq!(stored[0].status(), &ActionStatus::Proposed);
    assert_eq!(
        stored[0].chat_token_hash(),
        Some(&token_hash(tokens.get(&stored[0].id()).unwrap())),
        "the event carries the new action's token",
    );
}

#[tokio::test]
async fn a_cached_proposal_for_a_removed_skill_is_dropped() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let answer = report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    );
    w.primary.push_text(answer);
    let TriageOutcome::Report(first) = w.run().await else {
        panic!("the first run calls the model");
    };
    assert_eq!(
        first.proposals.len(),
        1,
        "the fresh run kept the proposal in the cache"
    );

    // The first incident closes, then the same problem files as a new open incident
    // after the reopen window.
    w.manager
        .dismiss(
            w.incident.id(),
            UserId::new(),
            "closed on the test".to_owned(),
        )
        .await
        .unwrap();
    w.clock.advance(SignedDuration::from_hours(7));
    settle().await;
    let again = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*w.store, &again).await.unwrap();
    add_signal(&w.store, &again, disk(92.1), w.now()).await;

    // The host lost the skill before the repeat incident was triaged.
    w.link.set_skills(host_id(), Vec::new());
    w.drain();
    let TriageOutcome::Cached { report, from } = w.triage.triage(&again).await else {
        panic!("the repeat uses the cache");
    };
    assert_eq!(from, w.incident.id());
    assert_eq!(
        w.primary.requests().len(),
        1,
        "the repeat never calls the model"
    );
    assert!(
        report.proposals.is_empty(),
        "the dropped proposal is not offered"
    );
    assert!(
        report.details.contains("not a skill on this host"),
        "the report says why it was dropped: {}",
        report.details,
    );
    assert!(w.tokens().is_empty(), "no chat token was minted");
    let orphaned = w
        .actions()
        .await
        .iter()
        .filter(|a| a.incident() == again.id())
        .count();
    assert_eq!(orphaned, 0, "no action was stored for the removed skill");
}

#[tokio::test]
async fn a_cached_report_does_not_duplicate_a_waiting_action() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    ));
    let TriageOutcome::Report(first) = w.run().await else {
        panic!("the first run calls the model");
    };
    assert_eq!(
        first.proposals.len(),
        1,
        "the fresh run offered the proposal"
    );
    assert_eq!(w.actions().await.len(), 1, "the proposal waits for a human");
    let first_tokens = w.tokens();
    assert_eq!(
        first_tokens.len(),
        1,
        "the fresh run handed a chat token to its card"
    );

    let earlier = resolved_incident(host_id(), "docker.container", Some("nginx"), w.now()).unwrap();
    IncidentRepo::insert(&*w.store, &earlier).await.unwrap();
    w.signal(disk(92.1)).await;
    w.drain();

    let TriageOutcome::Cached { report, from } = w.run().await else {
        panic!("the repeat uses the cache");
    };
    assert_eq!(from, earlier.id());
    assert_eq!(
        w.primary.requests().len(),
        1,
        "the repeat never calls the model"
    );
    assert!(
        report.proposals.is_empty(),
        "the waiting duplicate is not offered again"
    );
    let waiting = w.actions().await;
    assert_eq!(waiting.len(), 1, "the waiting action is not stored twice");
    let tokens = w.tokens();
    assert_eq!(
        tokens.len(),
        1,
        "the cached run hands its card one replacement chat token"
    );
    let replacement = tokens.get(&waiting[0].id()).unwrap();
    assert_eq!(
        waiting[0].chat_token_hash(),
        Some(&token_hash(replacement)),
        "the waiting action now stores the replacement token"
    );
    let rotated = first_tokens.get(&waiting[0].id()).unwrap();
    assert_ne!(
        waiting[0].chat_token_hash(),
        Some(&token_hash(rotated)),
        "the token the first card carried stops working once rotated"
    );
}

#[tokio::test]
async fn fresh_retriage_skips_the_repeat_cache_and_calls_the_model_again() {
    let mut w = world(Setup::default()).await;
    w.primary.push_text(report());
    let TriageOutcome::Report(first) = w.run().await else {
        panic!("the first run calls the model");
    };

    let earlier = resolved_incident(host_id(), "docker.container", Some("nginx"), w.now()).unwrap();
    IncidentRepo::insert(&*w.store, &earlier).await.unwrap();
    w.signal(disk(92.1)).await;
    w.drain();
    assert_eq!(
        w.run().await,
        TriageOutcome::Cached {
            report: first,
            from: earlier.id()
        }
    );
    assert_eq!(w.primary.requests().len(), 1, "the repeat uses the cache");

    w.primary.push_text(report());
    assert!(matches!(
        w.triage.retriage(&w.incident).await,
        TriageOutcome::Report(_)
    ));
    assert_eq!(
        w.primary.requests().len(),
        2,
        "a fresh retriage calls the model again"
    );
}

#[tokio::test]
async fn fallback_to_secondary_then_failed() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary
        .push_err(LlmError::Unavailable("overloaded".to_owned()));
    w.secondary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert_eq!(w.secondary.requests().len(), 1);

    w.primary
        .push_err(LlmError::RateLimited { retry_after: None });
    w.secondary
        .push_err(LlmError::Transport("connection reset".to_owned()));
    assert!(
        matches!(
            w.run().await,
            TriageOutcome::Failed { reason } if reason.contains("rate limited") && reason.contains("connection reset")
        ),
        "the failure lists why each provider failed"
    );

    w.primary.push_err(LlmError::Auth);
    assert!(
        matches!(w.run().await, TriageOutcome::Failed { reason } if reason.contains("authentication failed")),
        "refused credentials still try the fallback"
    );
    assert_eq!(
        w.secondary.requests().len(),
        3,
        "the fallback runs even for refused credentials"
    );
}

#[tokio::test]
async fn model_not_found_from_primary_falls_back_to_secondary() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary
        .push_err(LlmError::InvalidResponse("model not found".to_owned()));
    w.secondary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert_eq!(w.secondary.requests().len(), 1);
}

#[tokio::test]
async fn unusable_answer_falls_back_to_secondary_after_the_repair_turn() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary.push_text("I cannot help");
    w.primary.push_text("Still not a report");
    w.secondary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert_eq!(
        w.primary.requests().len(),
        2,
        "the repair turn runs before falling back"
    );
    assert_eq!(w.secondary.requests().len(), 1);
}

#[tokio::test]
async fn unusable_answers_from_every_provider_name_the_provider_that_failed() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary.push_text("I cannot help");
    w.primary.push_text("Still not a report");
    w.secondary.push_text("no report either");
    w.secondary.push_text("still not a report");
    assert!(
        matches!(
            w.run().await,
            TriageOutcome::Failed { reason }
                if reason.contains("scripted failed:") && reason.contains("; scripted failed:")
        ),
        "an unusable report still says which provider produced it, for every provider",
    );
}

#[tokio::test]
async fn triage_recovers_when_the_second_fallback_answers() {
    let w = world(Setup {
        secondary: true,
        third: true,
        ..Setup::default()
    })
    .await;
    w.primary
        .push_err(LlmError::Unavailable("first stop".to_owned()));
    w.secondary
        .push_err(LlmError::Transport("second stop".to_owned()));
    w.third.push_text(report());
    assert!(
        matches!(w.run().await, TriageOutcome::Report(_)),
        "the second fallback answers when the two providers before it fail",
    );
    assert_eq!(w.primary.requests().len(), 1, "the primary is tried first");
    assert_eq!(
        w.secondary.requests().len(),
        1,
        "the first fallback is tried next",
    );
    assert_eq!(
        w.third.requests().len(),
        1,
        "the second fallback is tried last and answers",
    );
}

#[tokio::test]
async fn triage_falls_back_through_secondary_then_fallback_model() {
    let w = world(Setup {
        secondary: true,
        third: true,
        ..Setup::default()
    })
    .await;
    w.primary
        .push_err(LlmError::Unavailable("first stop".to_owned()));
    w.secondary
        .push_err(LlmError::Transport("second stop".to_owned()));
    w.third.push_err(LlmError::Auth);
    let reason = match w.run().await {
        TriageOutcome::Failed { reason } => reason,
        TriageOutcome::Report(_) => "".to_owned(),
        TriageOutcome::Skipped { .. } => "".to_owned(),
        TriageOutcome::Cached { .. } => "".to_owned(),
    };
    assert!(
        reason.contains("unavailable: first stop")
            && reason.contains("transport error: second stop")
            && reason.contains("authentication failed"),
        "the failure lists why each of the three providers failed",
    );
    assert_eq!(w.primary.requests().len(), 1, "the primary is tried first");
    assert_eq!(
        w.secondary.requests().len(),
        1,
        "the secondary provider is tried after the primary",
    );
    assert_eq!(
        w.third.requests().len(),
        1,
        "the last fallback is tried before the run fails",
    );
}

#[tokio::test]
async fn three_failures_raise_triage_degraded_and_success_clears_it() {
    let settings = TriageSettings {
        per_hour_runs: 10,
        ..TriageSettings::default()
    };
    let mut w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    for _ in 0..2 {
        w.primary.push_err(LlmError::Unavailable("down".to_owned()));
        assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    }
    assert!(AdvisoryRepo::list_open(&*w.store).await.unwrap().is_empty());
    w.drain();

    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    w.run().await;
    let open = AdvisoryRepo::list_open(&*w.store).await.unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].kind, domovoi_core::AdvisoryKind::TriageDegraded);
    let degraded: Vec<_> = w
        .drain()
        .into_iter()
        .filter(|e| matches!(e.kind, EventKind::TriageDegraded { .. }))
        .collect();
    assert_eq!(degraded.len(), 1);
    assert_eq!(degraded[0].incident.id(), w.incident.id());

    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    w.run().await;
    assert!(
        !w.drain()
            .iter()
            .any(|e| matches!(e.kind, EventKind::TriageDegraded { .. })),
        "one notice, not one per failure"
    );

    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    assert!(AdvisoryRepo::list_open(&*w.store).await.unwrap().is_empty());
}

#[tokio::test]
async fn zero_hourly_cap_skips_once_and_never_retries() {
    let settings = TriageSettings {
        per_hour_runs: 0,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    assert!(matches!(
        w.run().await,
        TriageOutcome::Skipped {
            reason,
        } if reason.contains("per hour") && !reason.contains("runs again at")
    ));
    for _ in 0..5 {
        w.clock.advance(SignedDuration::from_secs(2));
        settle().await;
    }
    let skipped = count(&w, w.incident.id(), "triage.skipped").await;
    assert_eq!(skipped, 1, "a zero cap skips only once");
    assert_eq!(w.primary.requests().len(), 0, "no model call ever runs");
}

#[tokio::test]
async fn failed_retries_stop_at_the_limit_even_when_other_incidents_succeed() {
    let settings = TriageSettings {
        per_hour_runs: 100,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let other = open_incident(host_id(), "docker.container", Some("redis")).unwrap();
    IncidentRepo::insert(&*w.store, &other).await.unwrap();
    w.primary.push_err(LlmError::Unavailable("bad".to_owned()));
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    for i in 0..4 {
        w.primary.push_text(report());
        assert!(matches!(
            w.triage.triage(&other).await,
            TriageOutcome::Report(_)
        ));
        if i < 2 {
            w.primary.push_err(LlmError::Unavailable("bad".to_owned()));
        }
        w.clock.advance(SignedDuration::from_mins(61));
        settle().await;
    }
    let failed = count(&w, w.incident.id(), "triage.failed").await;
    assert_eq!(failed, FAILED_RETRY_LIMIT, "the chain stops at the limit");
    let reported = count(&w, other.id(), "triage.report").await;
    assert_eq!(reported, 4, "successes elsewhere do not help");
}

#[tokio::test]
async fn failed_retries_stop_at_the_limit_when_other_timeline_entries_land_between_them() {
    let settings = TriageSettings {
        per_hour_runs: 100,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let other = open_incident(host_id(), "docker.container", Some("redis")).unwrap();
    IncidentRepo::insert(&*w.store, &other).await.unwrap();
    w.primary.push_err(LlmError::Unavailable("bad".to_owned()));
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    for i in 0..8 {
        w.primary.push_text(report());
        assert!(matches!(
            w.triage.triage(&other).await,
            TriageOutcome::Report(_)
        ));
        if i < 2 {
            w.primary.push_err(LlmError::Unavailable("bad".to_owned()));
        }
        // What the manager writes each time the severity moves.
        IncidentRepo::append_event(
            &*w.store,
            &IncidentTimelineEntry {
                incident: w.incident.id(),
                at: w.now(),
                kind: "severity.changed".to_owned(),
                summary: "warning → critical".to_owned(),
                detail: serde_json::Value::Null,
            },
        )
        .await
        .unwrap();
        w.clock.advance(SignedDuration::from_mins(61));
        settle().await;
    }
    let failed = count(&w, w.incident.id(), "triage.failed").await;
    assert_eq!(
        failed, FAILED_RETRY_LIMIT,
        "non-triage entries do not reset the chain"
    );
}

#[tokio::test]
async fn a_tool_use_entry_does_not_stop_a_failed_retry() {
    let settings = TriageSettings {
        per_hour_runs: 100,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_err(LlmError::Unavailable("bad".to_owned()));
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));

    // A follow-up that used tools lands after the failed run.
    IncidentRepo::append_event(
        &*w.store,
        &IncidentTimelineEntry {
            incident: w.incident.id(),
            at: w.now(),
            kind: "triage.tools".to_owned(),
            summary: "follow-up used journal".to_owned(),
            detail: serde_json::json!({
                "during": "follow-up",
                "tools": [{"tool": "journal", "skill": null, "failed": false}],
            }),
        },
    )
    .await
    .unwrap();

    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;

    assert_eq!(count(&w, w.incident.id(), "triage.failed").await, 1);
    assert_eq!(
        count(&w, w.incident.id(), "triage.report").await,
        1,
        "the failure retry still runs after a tool-use entry"
    );
}

#[tokio::test]
async fn cap_retry_runs_even_after_an_old_failed_retry_chain() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;

    // Three failed runs exhaust the incident's failure retries.
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    for _ in 0..2 {
        w.primary.push_err(LlmError::Unavailable("down".to_owned()));
        w.clock.advance(SignedDuration::from_mins(61));
        settle().await;
    }
    let failed = count(&w, w.incident.id(), "triage.failed").await;
    assert_eq!(failed, FAILED_RETRY_LIMIT);

    // A non-triage entry lands, and a much later cap skip must still retry.
    IncidentRepo::append_event(
        &*w.store,
        &IncidentTimelineEntry {
            incident: w.incident.id(),
            at: w.now(),
            kind: "severity.changed".to_owned(),
            summary: "warning → critical".to_owned(),
            detail: serde_json::Value::Null,
        },
    )
    .await
    .unwrap();
    w.clock.advance(SignedDuration::from_hours(24));
    let other = open_incident(host_id(), "docker.container", Some("redis")).unwrap();
    IncidentRepo::insert(&*w.store, &other).await.unwrap();
    w.primary.push_text(report());
    assert!(matches!(
        w.triage.triage(&other).await,
        TriageOutcome::Report(_)
    ));
    assert!(matches!(w.run().await, TriageOutcome::Skipped { .. }));

    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let reports = count(&w, w.incident.id(), "triage.report").await;
    assert_eq!(
        reports, 1,
        "the cap retry is not blocked by the old failure chain"
    );
}

#[tokio::test]
async fn muted_incident_is_not_retried() {
    let w = world(Setup::default()).await;
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    let until = w.now().checked_add(SignedDuration::from_hours(24)).unwrap();
    w.manager
        .mute(w.incident.id(), until, UserId::new())
        .await
        .unwrap();

    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let requests = w.primary.requests().len();
    assert_eq!(requests, 1, "a muted incident is never retried");
    let chain = count(&w, w.incident.id(), "triage.failed").await;
    assert_eq!(chain, 1, "the retry chain stopped at the mute");
}

#[tokio::test]
async fn one_free_slot_retries_one_incident_without_new_skips() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let mut others = Vec::new();
    for name in ["a", "b", "c", "d"] {
        let other = open_incident(host_id(), "docker.container", Some(name)).unwrap();
        IncidentRepo::insert(&*w.store, &other).await.unwrap();
        others.push(other);
    }
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    for other in &others {
        assert!(matches!(
            w.triage.triage(other).await,
            TriageOutcome::Skipped { .. }
        ));
    }

    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let requests = w.primary.requests().len();
    assert_eq!(requests, 2, "one retry runs when one slot frees");
    let skipped = total_skips(&w, &others).await;
    assert_eq!(skipped, 4, "the rest wait, nobody skipped again");

    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let later = w.primary.requests().len();
    assert_eq!(later, 3, "the next incident retries in order");
    let waited = total_skips(&w, &others).await;
    assert_eq!(waited, 4, "waiting incidents are not skipped again");
}

#[tokio::test]
async fn waiting_incidents_take_the_next_slot_that_frees() {
    let settings = TriageSettings {
        per_hour_runs: 2,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let mut others = Vec::new();
    for name in ["b", "c", "d", "e"] {
        let other = open_incident(host_id(), "docker.container", Some(name)).unwrap();
        IncidentRepo::insert(&*w.store, &other).await.unwrap();
        others.push(other);
    }
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    w.clock.advance(SignedDuration::from_mins(30));
    w.primary.push_text(report());
    assert!(matches!(
        w.triage.triage(&others[0]).await,
        TriageOutcome::Report(_)
    ));
    for other in &others[1..] {
        assert!(matches!(
            w.triage.triage(other).await,
            TriageOutcome::Skipped { .. }
        ));
    }

    for _ in 0..4 {
        w.primary.push_text(report());
    }
    w.clock.advance(SignedDuration::from_mins(31)); // T+61: the first slot frees
    settle().await;
    let first = w.primary.requests().len();
    w.clock.advance(SignedDuration::from_mins(30)); // T+91: the slot freed at T+90 is due
    settle().await;
    let second = w.primary.requests().len();
    assert_eq!(
        (first, second),
        (3, 4),
        "the slot freed at T+90 is used instead of waiting another hour"
    );

    w.primary.push_text(report());
    w.clock.advance(SignedDuration::from_mins(31)); // T+122: the last waiter's slot is due
    settle().await;
    let last = w.primary.requests().len();
    assert_eq!(last, 5, "the last waiter runs when its slot frees");
    let skipped = total_skips(&w, &others[1..]).await;
    assert_eq!(skipped, 3, "the waiters add no new skips");
}

#[tokio::test]
async fn resolved_incident_is_not_retried() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let gone = resolved_incident(host_id(), "docker.container", Some("solved"), w.now()).unwrap();
    IncidentRepo::insert(&*w.store, &gone).await.unwrap();
    assert!(matches!(
        w.triage.triage(&gone).await,
        TriageOutcome::Skipped { .. }
    ));

    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let requests = w.primary.requests().len();
    assert_eq!(requests, 1, "a resolved incident is never retried");
    let skipped = count(&w, gone.id(), "triage.skipped").await;
    assert_eq!(skipped, 1, "only the explicit skip landed");
}

#[tokio::test]
async fn incident_triaged_by_a_newer_request_is_not_retried() {
    let w = world(Setup::default()).await;
    let other = open_incident(host_id(), "docker.container", Some("redis")).unwrap();
    IncidentRepo::insert(&*w.store, &other).await.unwrap();
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    assert!(matches!(
        w.triage.triage(&other).await,
        TriageOutcome::Failed { .. }
    ));

    w.clock.advance(SignedDuration::from_mins(30));
    w.primary.push_text(report());
    assert!(matches!(
        w.triage.triage(&other).await,
        TriageOutcome::Report(_)
    ));
    w.clock.advance(SignedDuration::from_mins(31));
    settle().await;
    let requests = w.primary.requests().len();
    assert_eq!(requests, 2, "a newer triage drops the pending retry");
    assert_eq!(count(&w, other.id(), "triage.report").await, 1);
    assert_eq!(count(&w, other.id(), "triage.failed").await, 1);
}

#[tokio::test]
async fn failed_run_is_not_retried_once_triage_is_degraded() {
    let settings = TriageSettings {
        per_hour_runs: 100,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let beta = open_incident(host_id(), "docker.container", Some("beta")).unwrap();
    IncidentRepo::insert(&*w.store, &beta).await.unwrap();
    let gamma = open_incident(host_id(), "docker.container", Some("gamma")).unwrap();
    IncidentRepo::insert(&*w.store, &gamma).await.unwrap();
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    assert!(matches!(
        w.triage.triage(&beta).await,
        TriageOutcome::Failed { .. }
    ));
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    assert!(matches!(
        w.triage.triage(&gamma).await,
        TriageOutcome::Failed { .. }
    ));

    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    w.primary.push_err(LlmError::Unavailable("down".to_owned()));
    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let requests = w.primary.requests().len();
    assert_eq!(requests, 5, "only the two deferred failures retry");
    assert_eq!(count(&w, w.incident.id(), "triage.failed").await, 2);
    assert_eq!(count(&w, beta.id(), "triage.failed").await, 2);
    let never = count(&w, gamma.id(), "triage.failed").await;
    assert_eq!(never, 1, "the third failure never deferred");

    w.clock.advance(SignedDuration::from_mins(61));
    settle().await;
    let again = w.primary.requests().len();
    assert_eq!(again, 5, "nothing retries again while degraded");
}

#[tokio::test]
async fn questions_count_against_the_hourly_limit() {
    let settings = TriageSettings {
        per_hour_runs: 3,
        reserved_for_incidents: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text("First answer.");
    w.primary.push_text("Second answer.");
    w.triage
        .follow_up(w.incident.id(), "First?", UserId::new())
        .await
        .unwrap();
    w.triage
        .follow_up(w.incident.id(), "Second?", UserId::new())
        .await
        .unwrap();
    assert_eq!(
        w.primary.requests().len(),
        2,
        "both questions used the model"
    );

    let refused = w
        .triage
        .follow_up(w.incident.id(), "Third?", UserId::new())
        .await;
    assert!(
        refused.is_err(),
        "the third question is refused at the hourly model limit",
    );
    let refused_at = match refused {
        Err(TriageError::Busy { retry_at: Some(at) }) => at,
        Err(err) => panic!("the refusal should be hourly busy: {err}"),
        Ok(answered) => panic!("the third question was not refused: {answered}"),
    };
    let freed = w
        .now()
        .checked_add(SignedDuration::from_hours(1))
        .and_then(|at| at.checked_add(SignedDuration::from_secs(1)))
        .unwrap();
    assert_eq!(
        refused_at, freed,
        "the refusal names when the hour frees an admissible slot",
    );
    assert_eq!(
        w.primary.requests().len(),
        2,
        "a refused question calls no model",
    );
}

#[tokio::test]
async fn questions_cannot_use_the_reserved_runs() {
    let settings = TriageSettings {
        per_hour_runs: 3,
        reserved_for_incidents: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_text("First answer.");
    w.primary.push_text("Second answer.");
    w.triage
        .follow_up(w.incident.id(), "First?", UserId::new())
        .await
        .unwrap();
    w.triage
        .follow_up(w.incident.id(), "Second?", UserId::new())
        .await
        .unwrap();
    let refused = w
        .triage
        .follow_up(w.incident.id(), "Third?", UserId::new())
        .await;
    assert!(
        matches!(refused, Err(TriageError::Busy { retry_at: Some(_) })),
        "the third question may not spend the run kept for triage",
    );
    assert_eq!(
        w.primary.requests().len(),
        2,
        "a refused question calls no model",
    );

    w.primary.push_text(report());
    assert!(
        matches!(w.run().await, TriageOutcome::Report(_)),
        "triage still gets the run the hour keeps free for it",
    );
    assert_eq!(
        w.primary.requests().len(),
        3,
        "the triage used the reserved run",
    );

    let again = w
        .triage
        .follow_up(w.incident.id(), "Fourth?", UserId::new())
        .await;
    assert!(
        matches!(again, Err(TriageError::Busy { retry_at: Some(_) })),
        "questions can never use the reserved runs of the hour",
    );
}

#[tokio::test]
async fn follow_up_is_refused_when_the_monthly_budget_is_spent() {
    let settings = TriageSettings {
        monthly_budget_micros: Some(1_000),
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    UsageRepo::record(&*w.store, "anthropic", 10, 10, 1_000, w.now())
        .await
        .unwrap();
    let refused = w
        .triage
        .follow_up(w.incident.id(), "Anything?", UserId::new())
        .await;
    assert_eq!(
        refused,
        Err(TriageError::BudgetSpent),
        "the question names the spent budget",
    );
    let refused_message = match refused {
        Err(err) => err.to_string(),
        Ok(answered) => answered,
    };
    assert_eq!(
        refused_message, "the monthly model budget is spent",
        "the message is the one the hub shows the operator",
    );
    assert!(
        w.primary.requests().is_empty(),
        "no model call for a spent budget"
    );
}

#[tokio::test]
async fn a_second_question_at_once_is_busy() {
    let settings = TriageSettings {
        concurrency: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.hold();
    w.primary.push_text("One answer.");
    let incident = w.incident.id();
    let triage = w.triage.clone();
    let running =
        tokio::spawn(async move { triage.follow_up(incident, "Running?", UserId::new()).await });
    w.primary.wait_for_request().await;
    let busy = w
        .triage
        .follow_up(incident, "Asked while another runs?", UserId::new())
        .await;
    assert!(
        matches!(busy, Err(TriageError::Busy { retry_at: None })),
        "the second question at once is refused as busy",
    );
    let busy_message = match busy {
        Err(err) => err.to_string(),
        Ok(answered) => answered,
    };
    assert_eq!(
        busy_message, "another question is being answered; try again in a moment",
        "the busy message says to try again shortly"
    );
    w.primary.release(1);
    let answered = running.await.unwrap().unwrap();
    assert_eq!(
        answered,
        "One answer.".to_owned(),
        "the question still running got its answer",
    );
    assert_eq!(
        w.primary.requests().len(),
        1,
        "the busy question never called the model",
    );
    assert_eq!(
        count(&w, incident, domovoi_engine::triage::FOLLOW_UP_KIND).await,
        1,
        "only the answered question lands on the timeline",
    );
}

#[tokio::test]
async fn a_question_refused_as_busy_takes_no_run_of_the_hour() {
    let settings = TriageSettings {
        concurrency: 1,
        per_hour_runs: 2,
        reserved_for_incidents: 0,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.hold();
    w.primary.push_text("One answer.");
    w.primary.push_text("Third answer.");
    let incident = w.incident.id();
    let triage = w.triage.clone();
    let running =
        tokio::spawn(async move { triage.follow_up(incident, "Running?", UserId::new()).await });
    w.primary.wait_for_request().await;
    let busy = w
        .triage
        .follow_up(incident, "Asked while another runs?", UserId::new())
        .await;
    assert!(
        matches!(busy, Err(TriageError::Busy { retry_at: None })),
        "the second question at once is refused as busy",
    );
    w.primary.release(1);
    assert_eq!(running.await.unwrap().unwrap(), "One answer.");

    // The busy refusal took no run: the hour of two still admits this one.
    w.primary.release(1);
    let answered = w
        .triage
        .follow_up(incident, "After the first finishes?", UserId::new())
        .await;
    assert_eq!(
        answered.unwrap(),
        "Third answer.",
        "the busy refusal left the second run of the hour free",
    );
    assert_eq!(
        w.primary.requests().len(),
        2,
        "only the two admitted questions called the model",
    );
}

#[tokio::test]
async fn an_unknown_incident_takes_no_run_of_the_hour() {
    let settings = TriageSettings {
        per_hour_runs: 1,
        reserved_for_incidents: 0,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let unknown = IncidentId::new();
    let refused = w
        .triage
        .follow_up(unknown, "What happened?", UserId::new())
        .await;
    assert!(
        matches!(refused, Err(TriageError::UnknownIncident(id)) if id == unknown),
        "an unknown incident is refused as unknown",
    );
    w.primary.push_text("One answer.");
    assert_eq!(
        w.triage
            .follow_up(w.incident.id(), "And this one?", UserId::new())
            .await
            .unwrap(),
        "One answer.",
        "the unknown incident took no run of the hour",
    );
    assert_eq!(w.primary.requests().len(), 1);
}

#[tokio::test]
async fn the_refusal_names_the_run_that_makes_room_for_a_question() {
    let settings = TriageSettings {
        per_hour_runs: 3,
        reserved_for_incidents: 1,
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    let mut others = Vec::new();
    for name in ["b", "c", "d"] {
        let other = open_incident(host_id(), "docker.container", Some(name)).unwrap();
        IncidentRepo::insert(&*w.store, &other).await.unwrap();
        others.push(other);
    }
    // Three triage runs ten minutes apart use the whole hour, including the run the hour
    // keeps for triage.
    for other in &others {
        w.primary.push_text(report());
        assert!(matches!(
            w.triage.triage(other).await,
            TriageOutcome::Report(_)
        ));
        w.clock.advance(SignedDuration::from_mins(10));
    }

    // A question may only use two of the three starts, so the second start has to leave
    // the hour before one fits, not the first.
    let refused = w
        .triage
        .follow_up(w.incident.id(), "Now?", UserId::new())
        .await;
    let retry_at = match refused {
        Err(TriageError::Busy { retry_at: Some(at) }) => at,
        Err(err) => panic!("the refusal should name when a run frees: {err}"),
        Ok(answer) => panic!("the question was not refused: {answer}"),
    };
    let expected = w
        .now()
        .checked_add(SignedDuration::from_mins(40))
        .and_then(|at| at.checked_add(SignedDuration::from_secs(1)))
        .unwrap();
    assert_eq!(
        retry_at, expected,
        "the retry waits for the start that leaves the question room",
    );
}
