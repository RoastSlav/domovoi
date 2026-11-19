use domovoi_core::ports::{IncidentRepo, LlmError, Part};
use domovoi_core::{CheckKey, ReadToolCall, TriageOutcome, UserId};
use domovoi_engine::triage::{MAX_ANSWER_CHARS, TriageError};
use domovoi_testkit::link::LinkRequest;
use serde_json::json;

use crate::harness::{Setup, calls, host_id, report, report_json, restart_skill, world};

#[tokio::test]
async fn cli_provider_gets_prefetched_context_and_no_tools() {
    let w = world(Setup {
        cli: true,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 1, "one shot");
    assert!(requests[0].tools.is_empty());
    let Part::Text(prompt) = &requests[0].messages[0].parts[0] else {
        panic!("the prompt is text");
    };
    for section in [
        "### docker_ps",
        "### docker_logs",
        "### journal",
        "### host_overview",
    ] {
        assert!(prompt.contains(section), "{section} missing");
    }
    assert!(prompt.contains("output of docker_logs"));
    assert!(w.link.requests().iter().any(|r| matches!(
        r,
        LinkRequest::Tool { call: ReadToolCall::DockerLogs { container, .. }, .. } if container == "nginx"
    )));
}

#[tokio::test]
async fn proposal_for_unknown_skill_or_bad_args_is_dropped() {
    let w = world(Setup::default()).await;
    let mut only_disks = restart_skill();
    only_disks.name = domovoi_core::SkillName::parse("clean_disk").unwrap();
    only_disks.applies_to = vec![CheckKey::parse("disk.usage").unwrap()];
    w.link
        .set_skills(host_id(), vec![restart_skill(), only_disks]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([
            {"skill": "wipe_disk", "args": {}, "reason": "x"},
            {"skill": "restart_container", "args": {"container": "../etc"}, "reason": "x"},
            {"skill": "clean_disk", "args": {"container": "nginx"}, "reason": "x"}
        ]),
    ));
    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert!(report.proposals.is_empty());
    assert!(w.actions().await.is_empty());
    for line in [
        "wipe_disk: not a skill on this host",
        "restart_container: bad arguments",
        "clean_disk: does not apply to docker.container",
    ] {
        assert!(
            report.details.contains(line),
            "{line} in {}",
            report.details
        );
    }
}

#[tokio::test]
async fn follow_up_that_ignores_the_proposal_protocol_creates_no_actions() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary.push_ok(calls(&[(
        "c1",
        "docker_logs",
        json!({"container": "nginx"}),
    )]));
    let long = format!(
        "It crashes on a bad config. {} {}",
        report_json(
            "ACTION_NEEDED",
            json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "x"}])
        ),
        "more ".repeat(1200)
    );
    w.primary.push_text(long);

    let answer = w
        .triage
        .follow_up(w.incident.id(), "Why does nginx restart?", UserId::new())
        .await
        .unwrap();
    assert!(answer.starts_with("It crashes on a bad config."));
    assert_eq!(answer.chars().count(), MAX_ANSWER_CHARS);
    assert_eq!(
        crate::harness::results(&w.primary.requests()[1]),
        vec![("output of docker_logs".to_owned(), false)]
    );
    assert!(
        w.actions().await.is_empty(),
        "an answer without a PROPOSALS: line proposes nothing"
    );
    assert!(
        !w.link
            .requests()
            .iter()
            .any(|r| matches!(r, LinkRequest::Skill { .. }))
    );
    let timeline = IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    let entry = timeline
        .iter()
        .find(|e| e.kind == "triage.followup")
        .expect("the follow-up is in the timeline");
    assert_eq!(entry.summary, "Why does nginx restart?");
    assert_eq!(entry.detail["answer"], answer);
}

#[tokio::test]
async fn follow_up_falls_back_on_refused_credentials() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary.push_err(LlmError::Auth);
    w.secondary.push_text("The config file is bad.");
    let answer = w
        .triage
        .follow_up(w.incident.id(), "Anything else?", UserId::new())
        .await
        .unwrap();
    assert!(answer.starts_with("The config file is bad."));
    assert_eq!(
        w.secondary.requests().len(),
        1,
        "the fallback answers the follow-up"
    );
}

#[tokio::test]
async fn follow_up_failure_names_each_failed_providers_reason() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary.push_err(LlmError::Auth);
    w.secondary
        .push_err(LlmError::Transport("connection reset".to_owned()));
    let outcome = w
        .triage
        .follow_up(w.incident.id(), "Anything else?", UserId::new())
        .await;
    assert!(
        matches!(
            outcome,
            Err(TriageError::Provider(reason))
                if reason.contains("authentication failed") && reason.contains("connection reset")
        ),
        "the follow-up failure lists why every provider failed, not just the last",
    );
}
