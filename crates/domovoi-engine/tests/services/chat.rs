//! A general chat over the running service: a scripted tool call reaching the fake agent
//! link of the host the operator chose, a model offered no tools when every host is asked
//! about, the earlier chat replayed before the new question, and a spent monthly budget
//! refusing with no model call at all.

use domovoi_core::ports::{Part, Presence, UsageRepo};
use domovoi_core::{HostId, ReadToolCall};
use domovoi_engine::triage::{ChatRole, ChatTurn, TriageError, TriageSettings};
use domovoi_testkit::build::T0;
use domovoi_testkit::link::LinkRequest;
use serde_json::json;
use uuid::Uuid;

use crate::harness::{Setup, calls, host_id, world};

/// A host other than the harness's one, so a tool call can be shown to run on the host the
/// operator chose rather than the one the incident was opened on.
fn other_host_id() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002))
}

#[tokio::test]
async fn a_chosen_hosts_tool_calls_run_on_that_host() {
    let w = world(Setup::default()).await;
    w.link.set_presence(
        other_host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
    w.primary
        .push_ok(calls(&[("c1", "journal", json!({"unit": "nginx"}))]));
    w.primary.push_text("The journal shows nothing slow.");

    let answer = w
        .triage
        .chat(
            Some(other_host_id()),
            "The NAS has two disks.",
            &Vec::new(),
            "Why is the service slow?",
        )
        .await
        .unwrap();
    assert_eq!(answer, "The journal shows nothing slow.".to_owned());

    let tool_calls: Vec<(HostId, ReadToolCall)> = w
        .link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Tool { host, call, .. } => Some((host, call)),
            _ => None,
        })
        .collect();
    assert_eq!(tool_calls.len(), 1, "the model made one tool call");
    assert_eq!(
        tool_calls[0].0,
        other_host_id(),
        "the journal call ran on the host the operator chose: {tool_calls:?}",
    );
    assert!(
        matches!(tool_calls[0].1, ReadToolCall::Journal { .. }),
        "it is the scripted journal call: {tool_calls:?}",
    );
}

#[tokio::test]
async fn asking_about_all_hosts_sends_the_model_no_tools() {
    let w = world(Setup::default()).await;
    w.primary.push_text("Both hosts look calm.");

    let answer = w
        .triage
        .chat(
            None,
            "Two hosts: the NAS and the pi.",
            &Vec::new(),
            "Anything wrong?",
        )
        .await
        .unwrap();
    assert_eq!(answer, "Both hosts look calm.".to_owned());

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 1, "one model call answers the question");
    assert!(
        requests[0].tools.is_empty(),
        "with no host chosen the model is offered no tools: {requests:?}",
    );
}

#[tokio::test]
async fn the_earlier_chat_comes_back_as_earlier_messages() {
    let w = world(Setup::default()).await;
    let history = vec![
        ChatTurn {
            role: ChatRole::User,
            text: "Why is the NAS slow?".to_owned(),
        },
        ChatTurn {
            role: ChatRole::Assistant,
            text: "Both disks are at 94%.".to_owned(),
        },
    ];
    w.primary.push_text("Still slow: the disks are filling.");

    w.triage
        .chat(None, "Two hosts.", &history, "Is it still bad?")
        .await
        .unwrap();

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 1, "the model answers once");
    let shown: Vec<String> = requests[0]
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        shown.len(),
        4,
        "the overview, the two earlier turns, the question"
    );
    assert_eq!(
        shown[0],
        "Two hosts.".to_owned(),
        "the host summary opens the request"
    );
    assert_eq!(
        shown[1],
        "Why is the NAS slow?".to_owned(),
        "the earlier question is replayed before the new one",
    );
    assert_eq!(
        shown[2],
        "Both disks are at 94%.".to_owned(),
        "its answer is replayed right after it",
    );
    assert_eq!(
        shown[3],
        "Is it still bad?".to_owned(),
        "the new question is the last message",
    );
}

#[tokio::test]
async fn a_spent_monthly_budget_refuses_the_chat_without_a_model_call() {
    let settings = TriageSettings {
        monthly_budget_micros: Some(1_000_000),
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    UsageRepo::record(&*w.store, "anthropic", 10, 10, 1_000_000, w.now())
        .await
        .unwrap();

    let refused = w
        .triage
        .chat(Some(host_id()), "The NAS.", &Vec::new(), "Why is it slow?")
        .await
        .err();
    assert!(
        matches!(
            &refused,
            Some(TriageError::Provider(reason)) if reason.contains("monthly"),
        ),
        "the spent monthly budget is the reason: {refused:?}",
    );
    assert!(
        w.primary.requests().is_empty(),
        "refusing the chat makes no model call"
    );
}
