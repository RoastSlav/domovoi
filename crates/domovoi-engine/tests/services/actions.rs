use std::collections::BTreeMap;

use domovoi_core::ports::{ActionRepo, AgentLink, IncidentRepo, LlmError, Part};
use domovoi_core::{
    ActionStatus, AgentSessionId, CheckKey, ExecutionId, ImplHash, KeyFingerprint, PlanContract,
    PlanHash, PlanTarget, Principal, ReadToolCall, TriageOutcome, UserId,
};
use domovoi_engine::triage::{MAX_ANSWER_CHARS, PROPOSAL_TTL, TriageError, token_hash};
use domovoi_testkit::build::open_incident;
use domovoi_testkit::link::LinkRequest;
use serde_json::json;

use crate::harness::{
    Setup, add_signal, calls, disk, host_id, report, report_json, restart_skill, world,
};

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
async fn valid_proposal_creates_proposed_action_with_hashed_chat_token() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    ));
    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert_eq!(report.proposals.len(), 1);

    let actions = w.actions().await;
    assert_eq!(actions.len(), 1);
    let action = &actions[0];
    assert_eq!(action.status(), &ActionStatus::Proposed);
    assert_eq!(action.proposed_by(), &Principal::System("triage".into()));
    assert_eq!(
        action.expires_at(),
        w.now().checked_add(PROPOSAL_TTL).unwrap()
    );

    let tokens = w.tokens();
    let token = tokens
        .get(&action.id())
        .expect("the event carries the token");
    assert_eq!(token.len(), 64);
    assert_eq!(action.chat_token_hash(), Some(&token_hash(token)));
    assert_ne!(action.chat_token_hash(), Some(&[0; 32]));
    let found = ActionRepo::find_by_chat_token_hash(&*w.store, &token_hash(token))
        .await
        .unwrap();
    assert_eq!(found.map(|a| a.id()), Some(action.id()));
}

#[tokio::test]
async fn running_triage_twice_with_the_same_proposal_leaves_one_proposed_action() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let answer = report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    );
    w.primary.push_text(answer.clone());
    w.primary.push_text(answer);

    let TriageOutcome::Report(first) = w.run().await else {
        panic!("a report");
    };
    assert_eq!(first.proposals.len(), 1);
    assert_eq!(w.actions().await.len(), 1);

    let TriageOutcome::Report(second) = w.triage.retriage(&w.incident).await else {
        panic!("a report");
    };
    assert_eq!(w.primary.requests().len(), 2, "the model ran again");
    assert!(
        second.proposals.is_empty(),
        "the duplicate is not offered again"
    );
    assert!(
        !second.details.contains("restart_container"),
        "a duplicate is not an error: {}",
        second.details
    );
    assert_eq!(
        w.actions().await.len(),
        1,
        "only one proposed action is stored"
    );
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

/// Gives the service's tasks turns to run their own awaits between clock jumps.
async fn settle() {
    for _ in 0..500 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_stable_agent_snapshot_stores_the_proposal_with_the_expected_v2_hash() {
    let w = world(Setup::default()).await;
    let skill = restart_skill();
    w.link.set_skills(host_id(), vec![skill.clone()]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    ));
    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert_eq!(report.proposals.len(), 1);

    let actions = w.actions().await;
    assert_eq!(actions.len(), 1);
    let action = &actions[0];
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let target = PlanTarget::of_incident(&w.incident, KeyFingerprint::of_public_key(&[7; 32]));
    let expected = PlanHash::compute_v2(
        &PlanContract::skill(&skill.name, 1).unwrap(),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &target,
        &args,
        ExecutionId::from_action(action.id()),
    );
    assert_eq!(
        action.plan_hash_v2(),
        Some(expected),
        "the proposal is bound to the session and key that supplied its catalogue",
    );
}

#[tokio::test]
async fn a_session_change_during_triage_stores_no_action() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let stable = w
        .link
        .snapshot(host_id())
        .expect("the harness has a live session");
    let mut changed = stable.clone();
    changed.session = AgentSessionId::new();
    w.link.script_snapshots([Some(stable), Some(changed)]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    ));

    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert!(report.proposals.is_empty());
    assert!(
        report
            .details
            .contains("the agent session changed while triage ran"),
        "the drop says why: {}",
        report.details,
    );
    assert!(
        w.actions().await.is_empty(),
        "no mismatched action is stored"
    );
}

#[tokio::test]
async fn an_agent_change_during_triage_stores_no_action() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let stable = w
        .link
        .snapshot(host_id())
        .expect("the harness has a live session");
    let mut changed = stable.clone();
    changed.agent = domovoi_core::AgentId::new();
    w.link.script_snapshots([Some(stable), Some(changed)]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    ));

    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert!(report.proposals.is_empty());
    assert!(
        report
            .details
            .contains("the agent session changed while triage ran"),
        "the drop says why: {}",
        report.details,
    );
    assert!(
        w.actions().await.is_empty(),
        "no mismatched action is stored"
    );
}

#[tokio::test]
async fn a_cached_report_with_a_changed_session_stores_no_action() {
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
    assert_eq!(first.proposals.len(), 1);
    w.drain();

    w.manager
        .dismiss(
            w.incident.id(),
            UserId::new(),
            "closed on the test".to_owned(),
        )
        .await
        .unwrap();
    w.clock.advance(jiff::SignedDuration::from_hours(7));
    settle().await;
    let again = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    IncidentRepo::insert(&*w.store, &again).await.unwrap();
    add_signal(&w.store, &again, disk(92.1), w.now()).await;

    let stable = w
        .link
        .snapshot(host_id())
        .expect("the harness has a live session");
    let mut changed = stable.clone();
    changed.session = AgentSessionId::new();
    w.link.script_snapshots([Some(stable), Some(changed)]);

    let TriageOutcome::Cached { report, .. } = w.triage.triage(&again).await else {
        panic!("the repeat reuses the cached report");
    };
    assert!(report.proposals.is_empty());
    assert!(
        report
            .details
            .contains("the agent session changed while triage ran"),
        "the cached regeneration says why: {}",
        report.details,
    );
    assert!(
        w.actions()
            .await
            .iter()
            .all(|action| action.incident() != again.id()),
        "a cached report from a replaced session stores no action",
    );
}
