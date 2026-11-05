use domovoi_core::TriageOutcome;
use domovoi_core::ports::LlmError;
use domovoi_core::ports::{AdvisoryRepo, IncidentRepo, UsageRepo};
use domovoi_engine::EventKind;
use domovoi_engine::triage::TriageSettings;
use domovoi_testkit::build::resolved_incident;
use jiff::SignedDuration;
use tokio::task::yield_now;

use crate::harness::{Setup, disk, host_id, report, world};

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
        TriageOutcome::Skipped { reason } if reason.contains("per hour")
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
        } if reason.contains("per hour") && reason.contains("runs again at")
    ));

    // The retry runs through the queue, so give the service's tasks turns to reach it.
    w.clock.advance(SignedDuration::from_mins(61));
    let mut yields = 0;
    while yields < 200 && w.primary.requests().len() < 2 {
        yield_now().await;
        yields += 1;
    }
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
    let mut yields = 0;
    while yields < 200 && w.primary.requests().len() < 2 {
        yield_now().await;
        yields += 1;
    }
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
    assert!(matches!(
        w.run().await,
        TriageOutcome::Failed { reason } if reason.contains("connection reset")
    ));

    w.primary.push_err(LlmError::Auth);
    assert!(matches!(w.run().await, TriageOutcome::Failed { .. }));
    assert_eq!(
        w.secondary.requests().len(),
        2,
        "refused credentials do not fall back"
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
