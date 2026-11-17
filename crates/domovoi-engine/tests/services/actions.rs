use std::collections::BTreeMap;

use domovoi_core::ports::{ActionRepo, AgentLink, HostRepo, IncidentRepo, LlmError, Part};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, AgentSessionId, CheckKey, ExecutionId, ImplHash,
    KeyFingerprint, PlanContract, PlanHash, PlanTarget, Principal, ReadToolCall, Risk,
    StoredAction, TriageOutcome, UserId,
};
use domovoi_engine::triage::{MAX_ANSWER_CHARS, PROPOSAL_TTL, TriageError, token_hash};
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;
use serde_json::json;

use crate::harness::{
    Setup, World, add_signal, calls, disk, host_id, report, report_json, restart_skill, world,
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

fn restart_proposal() -> String {
    report_json(
        "ACTION_NEEDED",
        json!([{"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}]),
    )
}

/// Runs one fresh triage that stores a single proposal.
async fn triage_once(w: &mut World) -> ActionRequest {
    w.primary.push_text(restart_proposal());
    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert_eq!(report.proposals.len(), 1, "the proposal is stored");
    let actions = w.actions().await;
    assert_eq!(actions.len(), 1);
    actions.into_iter().next().unwrap()
}

async fn triage_again(w: &mut World) -> TriageOutcome {
    w.primary.push_text(restart_proposal());
    w.run().await
}

#[tokio::test]
async fn an_unchanged_proposal_reuses_the_action_and_rotates_its_token() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();
    assert!(before.is_some(), "the first proposal carries a chat token");

    let TriageOutcome::Report(second) = triage_again(&mut w).await else {
        panic!("a report");
    };
    assert!(
        second.proposals.is_empty(),
        "the duplicate is a rotation, not a second action"
    );
    let actions = w.actions().await;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].id(), first.id());
    assert_ne!(
        actions[0].chat_token_hash(),
        before.as_ref(),
        "the waiting action's chat token rotated"
    );
}

#[tokio::test]
async fn a_changed_entry_script_stores_a_new_proposal() {
    let mut w = world(Setup::default()).await;
    let skill = restart_skill();
    w.link.set_skills(host_id(), vec![skill.clone()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();

    let mut changed = skill;
    changed.entry_script_digest = domovoi_core::ScriptDigest::of_bytes(b"a replaced entry");
    w.link.set_skills(host_id(), vec![changed]);
    let _ = triage_again(&mut w).await;

    let actions = w.actions().await;
    assert_eq!(
        actions.len(),
        2,
        "a different entry script is a different plan"
    );
    let old = actions.into_iter().find(|a| a.id() == first.id()).unwrap();
    assert_eq!(
        old.chat_token_hash(),
        before.as_ref(),
        "the stale action's token is not rotated"
    );
}

#[tokio::test]
async fn a_changed_manifest_stores_a_new_proposal() {
    let mut w = world(Setup::default()).await;
    let skill = restart_skill();
    w.link.set_skills(host_id(), vec![skill.clone()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();

    let mut changed = skill;
    changed.manifest_hash = "cd".repeat(32).parse().unwrap();
    w.link.set_skills(host_id(), vec![changed]);
    let _ = triage_again(&mut w).await;

    let actions = w.actions().await;
    assert_eq!(actions.len(), 2, "a different manifest is a different plan");
    let old = actions.into_iter().find(|a| a.id() == first.id()).unwrap();
    assert_eq!(old.chat_token_hash(), before.as_ref());
}

#[tokio::test]
async fn a_changed_incident_subject_stores_a_new_proposal() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();

    let changed = open_incident(host_id(), "docker.container", Some("/different"))
        .unwrap()
        .with_id(w.incident.id());
    IncidentRepo::update(&*w.store, &changed).await.unwrap();
    w.primary.push_text(restart_proposal());
    assert!(matches!(
        w.triage.triage(&changed).await,
        TriageOutcome::Report(_)
    ));

    let actions = w.actions().await;
    assert_eq!(actions.len(), 2, "a changed target is a different plan");
    let old = actions.into_iter().find(|a| a.id() == first.id()).unwrap();
    assert_eq!(old.chat_token_hash(), before.as_ref());
}

#[tokio::test]
async fn a_changed_enrolled_key_stores_a_new_proposal() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();

    let old_agent = HostRepo::live_agent(&*w.store, host_id())
        .await
        .unwrap()
        .expect("the harness host has an agent");
    HostRepo::revoke_agent(&*w.store, old_agent.agent, w.now())
        .await
        .unwrap();
    let mut replacement = old_agent.clone();
    replacement.agent = domovoi_core::AgentId::new();
    replacement.public_key = [9; 32];
    HostRepo::put_agent(&*w.store, &replacement).await.unwrap();
    w.link.set_agent(host_id(), replacement.agent);
    w.link.new_session(host_id());

    let _ = triage_again(&mut w).await;
    let actions = w.actions().await;
    assert_eq!(actions.len(), 2, "a changed key is a different plan");
    let old = actions.into_iter().find(|a| a.id() == first.id()).unwrap();
    assert_eq!(old.chat_token_hash(), before.as_ref());
}

#[tokio::test]
async fn a_changed_risk_stores_a_new_proposal() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();

    let mut riskier = restart_skill();
    riskier.risk = Risk::Destructive;
    w.link.set_skills(host_id(), vec![riskier]);
    let _ = triage_again(&mut w).await;

    let actions = w.actions().await;
    assert_eq!(
        actions.len(),
        2,
        "risk changes the approval requirement, so it is not identical"
    );
    let old = actions.into_iter().find(|a| a.id() == first.id()).unwrap();
    assert_eq!(old.chat_token_hash(), before.as_ref());
}

#[tokio::test]
async fn a_legacy_v1_proposal_is_not_reused_as_v2() {
    let mut w = world(Setup::default()).await;
    let skill = restart_skill();
    w.link.set_skills(host_id(), vec![skill.clone()]);
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let legacy = ActionRequest::try_from_stored(StoredAction {
        id: ActionId::new(),
        incident: w.incident.id(),
        host: host_id(),
        skill: skill.name.clone(),
        risk: skill.risk,
        args: args.to_map(),
        plan_hash: PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args),
        plan_hash_v2: None,
        approved_plan_hash: None,
        provenance: None,
        authorization_kind: None,
        status: ActionStatus::Proposed,
        proposed_by: Principal::System("triage".into()),
        approved_by: None,
        expires_at: T0.checked_add(SignedDuration::from_hours(1)).unwrap_or(T0),
        created_at: T0,
        updated_at: T0,
        chat_token_hash: Some([9; 32]),
    })
    .expect("a legacy proposal is a valid stored action");
    ActionRepo::insert(&*w.store, &legacy).await.unwrap();

    let TriageOutcome::Report(report) = triage_again(&mut w).await else {
        panic!("a report");
    };
    assert_eq!(
        report.proposals.len(),
        1,
        "a v1 action is not the same plan"
    );
    let actions = w.actions().await;
    assert_eq!(actions.len(), 2);
    let old = actions.into_iter().find(|a| a.id() == legacy.id()).unwrap();
    assert_eq!(
        old.chat_token_hash(),
        Some(&[9; 32]),
        "the legacy action's token is not rotated"
    );
}

#[tokio::test]
async fn a_session_change_before_matching_does_not_rotate_the_token() {
    let mut w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let first = triage_once(&mut w).await;
    let before = first.chat_token_hash().copied();

    let stable = w
        .link
        .snapshot(host_id())
        .expect("the harness has a live session");
    let mut changed = stable.clone();
    changed.session = AgentSessionId::new();
    w.link.script_snapshots([Some(stable), Some(changed)]);

    let TriageOutcome::Report(report) = triage_again(&mut w).await else {
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
    let actions = w.actions().await;
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].chat_token_hash(),
        before.as_ref(),
        "a replaced session does not rotate the stale action's token"
    );
}
