//! `chat_streaming` over the running service: the text and tools come back with the answer,
//! the sink is told per model request, and the Operator's model stays read-only.

use std::sync::{Arc, Mutex};

use domovoi_core::meta::{MetaTool, ToolSet};
use domovoi_core::ports::{AnswerSink, LlmResponse, Part, Presence, StopReason, ToolPhase, Usage};
use domovoi_testkit::build::T0;
use serde_json::json;

use crate::harness::{Setup, host_id, read_calls, world};

/// What a sink was told, in order.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Attempt,
    Text(String),
    Tool(String, ToolPhase, bool),
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Seen>>);

impl Recorder {
    fn seen(&self) -> Vec<Seen> {
        self.push(None)
    }

    /// Appends `item` when given and returns everything seen so far.
    fn push(&self, item: Option<Seen>) -> Vec<Seen> {
        let mut seen = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.extend(item);
        seen.clone()
    }
}

impl AnswerSink for Recorder {
    fn attempt_started(&self) {
        self.push(Some(Seen::Attempt));
    }
    fn text(&self, fragment: &str) {
        self.push(Some(Seen::Text(fragment.to_owned())));
    }
    fn tool(&self, tool: &str, phase: ToolPhase, failed: bool) {
        self.push(Some(Seen::Tool(tool.to_owned(), phase, failed)));
    }
}

async fn connected(w: &crate::harness::World) {
    w.link.set_presence(
        host_id(),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
}

#[tokio::test]
async fn chat_streaming_returns_the_text_and_the_tools_used() {
    let w = world(Setup::default()).await;
    connected(&w).await;
    w.primary
        .push_ok(read_calls(&[("c1", "journal", json!({"unit": "nginx"}))]));
    w.primary.push_text("Two restarts.");
    let sink = Arc::new(Recorder::default());

    let answer = w
        .triage
        .chat_streaming(Some(host_id()), "One host.", &[], "Why?", sink)
        .await
        .unwrap();

    assert_eq!(answer.text, "Two restarts.");
    assert_eq!(answer.tools.len(), 1, "{:?}", answer.tools);
    assert_eq!(answer.tools[0].tool, "journal");
    assert!(!answer.tools[0].failed);
}

#[tokio::test]
async fn chat_streaming_streams_fragments_and_resets_per_request() {
    let w = world(Setup::default()).await;
    connected(&w).await;
    w.primary
        .push_ok(read_calls(&[("c1", "journal", json!({"unit": "nginx"}))]));
    w.primary.push_streamed(&["Two ", "restarts."]);
    let sink = Arc::new(Recorder::default());

    let answer = w
        .triage
        .chat_streaming(Some(host_id()), "One host.", &[], "Why?", sink.clone())
        .await
        .unwrap();
    assert_eq!(answer.text, "Two restarts.");

    let seen = sink.seen();
    let attempts = seen.iter().filter(|s| **s == Seen::Attempt).count();
    assert_eq!(attempts, 2, "one attempt per model request: {seen:?}");
    assert_eq!(seen[0], Seen::Attempt);
    let started = seen
        .iter()
        .position(|s| matches!(s, Seen::Tool(_, ToolPhase::Started, _)))
        .expect("a tool started");
    let finished = seen
        .iter()
        .position(|s| matches!(s, Seen::Tool(_, ToolPhase::Finished, _)))
        .expect("a tool finished");
    let second_attempt = seen
        .iter()
        .rposition(|s| *s == Seen::Attempt)
        .expect("a second attempt");
    assert!(started < finished && finished < second_attempt, "{seen:?}");
    assert_eq!(
        &seen[second_attempt + 1..],
        [
            Seen::Text("Two ".to_owned()),
            Seen::Text("restarts.".to_owned())
        ],
        "the answer's fragments follow the last attempt start",
    );
}

#[tokio::test]
async fn chat_streaming_discards_an_incomplete_turn_and_restarts_the_text() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(LlmResponse {
        parts: vec![Part::Text("Half an ans".to_owned())],
        stop: StopReason::MaxTokens,
        usage: Usage::default(),
    });
    w.primary.push_text("Whole answer.");
    let sink = Arc::new(Recorder::default());

    let answer = w
        .triage
        .chat_streaming(None, "Hosts.", &[], "Status?", sink.clone())
        .await
        .unwrap();
    assert_eq!(answer.text, "Whole answer.");
    assert_eq!(
        sink.seen(),
        vec![
            Seen::Attempt,
            Seen::Text("Half an ans".to_owned()),
            Seen::Attempt,
            Seen::Text("Whole answer.".to_owned()),
        ],
        "the sink is told the first text was discarded before the retry",
    );
}

#[tokio::test]
async fn chat_and_chat_streaming_agree() {
    let plain = world(Setup::default()).await;
    plain.primary.push_text("All calm.");
    let a = plain
        .triage
        .chat(None, "Hosts.", &[], "Status?")
        .await
        .unwrap();

    let streamed = world(Setup::default()).await;
    streamed.primary.push_text("All calm.");
    let b = streamed
        .triage
        .chat_streaming(
            None,
            "Hosts.",
            &[],
            "Status?",
            Arc::new(Recorder::default()),
        )
        .await
        .unwrap();
    assert_eq!(a, b.text);
}

#[tokio::test]
async fn the_operator_model_is_offered_only_read_tools() {
    let w = world(Setup::default()).await;
    connected(&w).await;
    w.primary.push_text("Fine.");
    w.triage
        .chat_streaming(
            Some(host_id()),
            "One host.",
            &[],
            "Status?",
            Arc::new(Recorder::default()),
        )
        .await
        .unwrap();
    let requests = w.primary.requests();
    let offered: Vec<&str> = requests[0].tools.iter().map(|t| t.name.as_str()).collect();
    let read: Vec<&str> = ToolSet::Read.tools().iter().map(MetaTool::name).collect();
    assert_eq!(
        offered, read,
        "a streamed chat offers exactly the read tool set"
    );
}

#[tokio::test]
async fn a_model_that_names_approve_in_the_operator_changes_nothing() {
    let w = world(Setup::default()).await;
    connected(&w).await;
    w.primary.push_ok(LlmResponse {
        parts: vec![Part::ToolCall {
            id: "c1".to_owned(),
            name: "approve_action".to_owned(),
            input: json!({"action": "restart"}),
        }],
        stop: StopReason::ToolUse,
        usage: Usage::default(),
    });
    w.primary.push_text("Done.");
    let sink = Arc::new(Recorder::default());
    let answer = w
        .triage
        .chat_streaming(Some(host_id()), "One host.", &[], "Approve it", sink)
        .await
        .unwrap();
    assert_eq!(answer.text, "Done.");
    assert_eq!(answer.tools.len(), 1);
    assert_eq!(answer.tools[0].tool, "unknown tool");
    let request = &w.primary.requests()[1];
    let results: Vec<String> = request
        .messages
        .iter()
        .flat_map(|m| m.parts.iter())
        .filter_map(|p| match p {
            Part::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(results, ["error: unknown tool `approve_action`"]);
}
