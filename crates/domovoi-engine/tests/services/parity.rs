//! The queue-free triage run and the investigation operation give the same answers: the
//! same report, the same limits, the same refusals and the same safety. Each scenario runs
//! once in each mode and compares what an operator could see. This suite is the launch gate
//! for removing the queue.

use std::fmt::Debug;
use std::future::Future;

use domovoi_core::ports::{IncidentRepo, LlmResponse, NoteRepo, StopReason, ToolOutput, Usage};
use domovoi_core::{HostId, Incident, Note, NoteId, TriageBudget, TriageOutcome, TriageReport};
use domovoi_engine::operations::APPROVAL_POINTER;
use domovoi_engine::triage::TriageSettings;
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::LinkRequest;
use serde_json::{Value, json};

use crate::harness::{
    Mode, SECRET, Setup, World, add_signal, calls, disk, host_id, report, restart_skill,
    seen as text_seen, world,
};

/// Both modes, in the order scenarios run them.
pub const MODES: [Mode; 2] = [Mode::Legacy, Mode::Operations];

/// A setup for `mode` with everything else default.
pub fn setup(mode: Mode) -> Setup {
    Setup {
        mode,
        ..Setup::default()
    }
}

/// Runs `scenario` in both modes and requires equal results.
pub async fn both<T, F, Fut>(scenario: F) -> T
where
    T: PartialEq + Debug,
    F: Fn(Mode) -> Fut,
    Fut: Future<Output = T>,
{
    let legacy = scenario(Mode::Legacy).await;
    let operations = scenario(Mode::Operations).await;
    assert_eq!(legacy, operations, "the two paths disagree");
    legacy
}

/// `details` without the sentence an investigation adds while a step waits for a person.
fn plain_details(details: &str) -> String {
    details
        .strip_suffix(APPROVAL_POINTER)
        .unwrap_or(details)
        .trim_end()
        .to_owned()
}

/// A report with the one field only an investigation changes put back.
pub fn plain_report(report: &TriageReport) -> TriageReport {
    let mut plain = report.clone();
    plain.details = plain_details(&plain.details);
    plain
}

/// An outcome as an operator sees it, without ids and without the approval pointer.
pub fn shape(outcome: &TriageOutcome) -> Value {
    match outcome {
        TriageOutcome::Report(report) => {
            json!({"report": serde_json::to_value(plain_report(report)).unwrap()})
        }
        TriageOutcome::Cached { report, .. } => {
            json!({"cached": serde_json::to_value(plain_report(report)).unwrap()})
        }
        TriageOutcome::Skipped { reason } => json!({"skipped": reason}),
        TriageOutcome::Failed { reason } => json!({"failed": reason}),
    }
}

/// What a run left for an operator to look at.
#[derive(Debug, PartialEq)]
pub struct Seen {
    pub shape: Value,
    pub model_requests: usize,
    /// The tool calls the model made, as the incident's `triage.tools` entry records them.
    pub tools: Option<Value>,
    /// The triage entries of the incident's timeline, oldest first.
    pub kinds: Vec<String>,
}

/// The timeline kinds of `incident` that belong to triage.
pub async fn kinds(w: &World, incident: &Incident) -> Vec<String> {
    IncidentRepo::timeline(&*w.store, incident.id())
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.kind)
        .filter(|kind| kind.starts_with("triage."))
        .collect()
}

/// What `outcome` and the world's state show for the world's incident.
pub async fn seen(w: &World, outcome: &TriageOutcome) -> Seen {
    let timeline = IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    Seen {
        shape: shape(outcome),
        model_requests: w.primary.requests().len(),
        tools: timeline
            .iter()
            .find(|entry| entry.kind == "triage.tools")
            .map(|entry| entry.detail.clone()),
        kinds: kinds(w, &w.incident).await,
    }
}

/// A second open incident on the same host with its own target.
pub async fn another(w: &World, target: &str) -> Incident {
    let incident = open_incident(host_id(), "docker.container", Some(target)).unwrap();
    IncidentRepo::insert(&*w.store, &incident).await.unwrap();
    add_signal(&w.store, &incident, disk(40.0), w.now()).await;
    incident
}

/// Lets an investigation that has nothing left to do close, so a second one may start.
/// Does nothing in `Mode::Legacy`.
pub async fn quiesce(w: &World, mode: Mode, incident: &Incident) {
    if mode == Mode::Legacy {
        return;
    }
    for _ in 0..40 {
        let open = w
            .snapshots(incident)
            .await
            .iter()
            .any(|s| !s.operation.status.is_terminal());
        if !open {
            return;
        }
        w.drive(1, 31).await;
    }
    panic!("the investigation never closed");
}

/// A response that ended at the output limit without saying anything.
pub fn reasoning_only() -> LlmResponse {
    LlmResponse {
        parts: Vec::new(),
        stop: StopReason::MaxTokens,
        usage: Usage::default(),
    }
}

/// Every tool call the agent received, in order.
pub fn tool_requests(w: &World) -> Vec<(HostId, String)> {
    w.link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Tool { host, call, .. } => Some((host, format!("{call:?}"))),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn report_fields_are_identical_across_modes() {
    let answer = json!({
        "headline": "nginx keeps restarting",
        "summary": "The container exits a few seconds after start.",
        "cause": "A bad config file, fairly sure.",
        "evidence": [
            {"label": "exit code", "value": "1", "source": "docker_logs"},
            {"label": "restarts", "value": "14", "source": "docker_inspect"},
        ],
        "verdict": "ACTION_NEEDED",
        "next_step": "Fix the config file and restart the container.",
        "manual_steps": ["Open /etc/nginx/nginx.conf", "Reload the container"],
        "proposals": [
            {"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."},
            {"skill": "wipe_disk", "args": {}, "reason": "Make room."},
        ],
        "details": "The log shows a syntax error on line 12."
    })
    .to_string();
    let run = |mode: Mode| {
        let answer = answer.clone();
        async move {
            let w = world(setup(mode)).await;
            w.link.set_skills(host_id(), vec![restart_skill()]);
            w.primary.push_text(answer);
            let outcome = w.run(mode).await;
            let TriageOutcome::Report(report) = &outcome else {
                panic!("{mode:?}: {outcome:?}");
            };
            let proposed: Vec<_> = report
                .proposals
                .iter()
                .map(|p| (p.skill.clone(), p.args.clone()))
                .collect();
            let views: Vec<_> = w
                .proposals(mode, &w.incident)
                .await
                .into_iter()
                .map(|p| (p.skill, p.args, p.awaiting_human, p.approved))
                .collect();
            (seen(&w, &outcome).await, proposed, views)
        }
    };
    let (seen, proposed, views) = both(run).await;
    assert_eq!(proposed.len(), 1, "only the skill on the host is offered");
    assert_eq!(
        views.len(),
        1,
        "one waiting proposal, as an action or a step"
    );
    assert!(views[0].2 && !views[0].3, "waiting, not approved");
    let report = &seen.shape["report"];
    assert_eq!(report["headline"], "nginx keeps restarting");
    assert!(
        report["details"]
            .as_str()
            .unwrap()
            .ends_with("Proposals not offered:\n- wipe_disk: not a skill on this host"),
        "{}",
        report["details"]
    );
}

#[tokio::test]
async fn tool_call_budget_is_enforced_the_same() {
    let run = |mode: Mode| async move {
        let w = world(setup(mode)).await;
        let ids: Vec<String> = (1..=20).map(|n| format!("c{n}")).collect();
        let list: Vec<(&str, &str, Value)> = ids
            .iter()
            .map(|id| (id.as_str(), "docker_ps", json!({})))
            .collect();
        w.primary.push_ok(calls(&list));
        w.primary.push_text(report());
        let outcome = w.run(mode).await;
        assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
        let tools = tool_requests(&w)
            .into_iter()
            .filter(|(_, call)| call.starts_with("DockerPs"))
            .count();
        (seen(&w, &outcome).await, tools)
    };
    let (seen, docker_ps) = both(run).await;
    let entry = seen.tools.expect("the tool use is recorded");
    let refused = entry["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["refused"] == true)
        .count();
    assert_eq!(entry["tools"].as_array().unwrap().len(), 20);
    assert_eq!(refused, 5, "the model got 15 of its 20 calls");
    assert!(docker_ps >= 15, "the agent ran the 15 allowed calls");
    assert_eq!(seen.model_requests, 2);
}

#[tokio::test]
async fn output_token_budget_and_deadline_are_enforced_the_same() {
    // The run's total output budget runs out on the retry.
    let spent = |mode: Mode| async move {
        let settings = TriageSettings {
            budget: TriageBudget {
                max_tool_calls: 15,
                max_seconds: 90,
                max_output_tokens: 2000,
                max_total_output_tokens: 2500,
            },
            ..TriageSettings::default()
        };
        let w = world(Setup {
            mode,
            settings,
            ..Setup::default()
        })
        .await;
        w.primary.push_ok(reasoning_only());
        w.primary.push_ok(reasoning_only());
        let outcome = w.run(mode).await;
        let asked: Vec<u32> = w
            .primary
            .requests()
            .iter()
            .map(|r| r.max_output_tokens)
            .collect();
        (seen(&w, &outcome).await, asked)
    };
    let (seen_spent, asked) = both(spent).await;
    assert_eq!(
        asked,
        [2000, 500],
        "a retry never asks for more than is left"
    );
    assert!(
        seen_spent.shape["failed"]
            .as_str()
            .unwrap()
            .contains("output token budget")
    );

    // A deadline that has already passed sends no request.
    let late = |mode: Mode| async move {
        let settings = TriageSettings {
            budget: TriageBudget {
                max_seconds: 0,
                ..TriageSettings::default().budget
            },
            ..TriageSettings::default()
        };
        let w = world(Setup {
            mode,
            settings,
            ..Setup::default()
        })
        .await;
        let outcome = w.run(mode).await;
        seen(&w, &outcome).await
    };
    let seen_late = both(late).await;
    assert_eq!(seen_late.model_requests, 0);
    assert!(
        seen_late.shape["failed"]
            .as_str()
            .unwrap()
            .contains("run deadline")
    );
}

#[tokio::test(start_paused = true)]
async fn a_slow_model_is_cut_off_the_same_way_in_both_modes() {
    let run = |mode: Mode| async move {
        let w = world(setup(mode)).await;
        w.primary.hold();
        let incident = w.incident.clone();
        let task = w.spawn_run(mode, &incident);
        // The request is inside the provider; the run's time then runs out.
        for _ in 0..3000 {
            w.drive(1, 1).await;
            if !w.primary.requests().is_empty() {
                break;
            }
        }
        assert_eq!(
            w.primary.requests().len(),
            1,
            "{mode:?}: the model was asked"
        );
        tokio::time::advance(std::time::Duration::from_secs(91)).await;
        let outcome = w.finish(task).await;
        seen(&w, &outcome).await
    };
    let seen = both(run).await;
    assert!(
        seen.shape["failed"]
            .as_str()
            .unwrap()
            .contains("run deadline"),
        "{:?}",
        seen.shape
    );
}

#[tokio::test]
async fn secret_never_reaches_the_model_in_either_mode() {
    let run = |mode: Mode| async move {
        let mut evidence = disk(92.4);
        evidence.insert(
            "env",
            domovoi_core::EvidenceValue::Text(format!("DB_PASSWORD={SECRET}")),
        );
        let w = world(Setup {
            mode,
            evidence,
            ..Setup::default()
        })
        .await;
        w.note("db", &format!("the password is {SECRET}")).await;
        w.link.on_tool(|_, _| {
            Ok(ToolOutput {
                text: format!("connecting with {SECRET} and token=abc123"),
                truncated: false,
            })
        });
        w.primary.push_ok(calls(&[
            ("c1", "docker_logs", json!({"container": "nginx"})),
            ("c2", "notes_search", json!({"query": "password"})),
        ]));
        w.primary.push_text(report());
        let outcome = w.run(mode).await;
        for request in w.primary.requests() {
            let text = text_seen(&request);
            assert!(
                !text.contains(SECRET),
                "{mode:?}: a secret reached the model"
            );
            assert!(
                !text.contains("abc123"),
                "{mode:?}: a token reached the model"
            );
        }
        // Nor does it reach what was stored for the operator.
        let mut stored = format!("{outcome:?}");
        for snapshot in w.snapshots(&w.incident).await {
            stored.push_str(&format!("{:?}{:?}", snapshot.steps, snapshot.operation));
        }
        assert!(!stored.contains(SECRET), "{mode:?}: a secret was stored");
        seen(&w, &outcome).await
    };
    let seen = both(run).await;
    assert_eq!(seen.model_requests, 2);
}

#[tokio::test]
async fn host_scoping_is_the_same() {
    let run = |mode: Mode| async move {
        let w = world(setup(mode)).await;
        // Another host's note names a marker the model must never be shown.
        let other = Note {
            id: NoteId::new(),
            host: Some(HostId::new()),
            title: "elsewhere".to_owned(),
            body_markdown: "OTHERHOSTMARKER restart everything".to_owned(),
            tags: Vec::new(),
            updated_at: T0,
        };
        NoteRepo::upsert(&*w.store, &other).await.unwrap();
        w.primary.push_ok(calls(&[
            ("c1", "notes_search", json!({"query": "OTHERHOSTMARKER"})),
            ("c2", "docker_ps", json!({})),
        ]));
        w.primary.push_text(report());
        let outcome = w.run(mode).await;
        assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
        let leaked = w
            .primary
            .requests()
            .iter()
            .any(|request| text_seen(request).contains("OTHERHOSTMARKER restart"));
        let hosts: Vec<HostId> = tool_requests(&w)
            .into_iter()
            .map(|(host, _)| host)
            .collect();
        (seen(&w, &outcome).await, leaked, hosts)
    };
    let (_, leaked, hosts) = both(run).await;
    assert!(!leaked, "another host's note reached the model");
    assert!(!hosts.is_empty());
    assert!(
        hosts.iter().all(|host| *host == host_id()),
        "a read reached another host"
    );
}
