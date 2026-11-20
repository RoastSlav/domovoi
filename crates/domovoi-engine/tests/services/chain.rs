//! The provider-chain loop shared by triage, follow-up and chat, and the same-output note.

use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use domovoi_core::ports::{LlmCapability, LlmError, LlmProvider, LlmRequest, ToolOutput};
use domovoi_core::{ReadToolCall, StepKind, StepOutcome, TriageOutcome, UserId};
use domovoi_engine::triage::{NO_CALLS_LEFT, TriageError};
use domovoi_llm::{ProviderKind, ProviderSettings, build};
use domovoi_testkit::build::T0;
use domovoi_testkit::link::LinkRequest;
use jiff::Timestamp;
use serde_json::{Value, json};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::harness::{SECRET, Setup, World, host_id, read_calls, report, results, world};

/// Installs the rustls provider once, as the binary does at startup.
fn crypto() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A real OpenAI-compatible provider pointed at `base`.
fn provider(base: &str) -> Arc<dyn LlmProvider> {
    crypto();
    let mut settings = ProviderSettings::new(ProviderKind::OpenaiCompatible);
    settings.base_url = Some(Url::parse(&format!("{base}/v1")).unwrap());
    settings.model = Some("model-under-test".to_owned());
    build(&settings, None).unwrap()
}

fn tools_refused() -> LlmError {
    LlmError::CapabilityRejected {
        capability: LlmCapability::Tools,
        detail: "tools are not supported".to_owned(),
    }
}

fn down(label: &str) -> LlmError {
    LlmError::Unavailable(label.to_owned())
}

async fn world_with_fallbacks() -> World {
    world(Setup {
        secondary: true,
        third: true,
        ..Setup::default()
    })
    .await
}

async fn follow(w: &World) -> Result<String, TriageError> {
    w.triage
        .follow_up(w.incident.id(), "Anything new?", UserId::new())
        .await
}

async fn chatted(w: &World) -> Result<String, TriageError> {
    w.triage
        .chat(Some(host_id()), "One host.", &[], "How is it?")
        .await
}

fn provider_reason(result: Result<String, TriageError>) -> String {
    match result {
        Err(TriageError::Provider(reason)) => reason,
        other => panic!("not a provider failure: {other:?}"),
    }
}

fn failed_reason(outcome: TriageOutcome) -> String {
    match outcome {
        TriageOutcome::Failed { reason } => reason,
        other => panic!("not a failure: {other:?}"),
    }
}

#[tokio::test]
async fn the_chain_returns_the_first_success_and_asks_no_further_provider() {
    let w = world_with_fallbacks().await;
    w.primary.push_err(down("primary is down"));
    w.secondary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    assert_eq!(w.primary.requests().len(), 1);
    assert_eq!(w.secondary.requests().len(), 1);
    assert!(w.third.requests().is_empty(), "triage asked a third");

    let w = world_with_fallbacks().await;
    w.primary.push_err(down("primary is down"));
    w.secondary.push_text("The second answers.");
    assert_eq!(follow(&w).await.unwrap(), "The second answers.");
    assert_eq!(w.primary.requests().len(), 1);
    assert_eq!(w.secondary.requests().len(), 1);
    assert!(w.third.requests().is_empty(), "follow-up asked a third");

    let w = world_with_fallbacks().await;
    w.primary.push_err(down("primary is down"));
    w.secondary.push_text("The second answers.");
    assert_eq!(chatted(&w).await.unwrap(), "The second answers.");
    assert_eq!(w.primary.requests().len(), 1);
    assert_eq!(w.secondary.requests().len(), 1);
    assert!(w.third.requests().is_empty(), "chat asked a third");
}

/// The tool calls the run recorded in the incident's `triage.tools` entry.
async fn recorded_tools(w: &World) -> Vec<Value> {
    let timeline = domovoi_core::ports::IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    timeline
        .iter()
        .find(|e| e.kind == "triage.tools")
        .map(|e| e.detail["tools"].as_array().cloned().unwrap_or_default())
        .unwrap_or_default()
}

fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

#[tokio::test]
async fn a_capability_rejection_rebuilds_once_per_capability() {
    // A provider that refuses tools once is asked again without them, from the top, and the
    // rebuilt prompt carries the output the model can no longer gather itself.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|request: &Request| {
            if body_of(request).get("tools").is_some() {
                ResponseTemplate::new(400).set_body_string("tools are not supported by this model")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {"role": "assistant", "content": report()},
                        "finish_reason": "stop"
                    }]
                }))
            }
        })
        .mount(&server)
        .await;
    let w = world(Setup {
        provider: Some(provider(&server.uri())),
        ..Setup::default()
    })
    .await;
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "one refusal, one rebuilt attempt");
    assert!(body_of(&requests[0]).get("tools").is_some());
    let rebuilt = body_of(&requests[1]);
    assert!(rebuilt.get("tools").is_none());
    assert!(
        rebuilt["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["content"]
                .as_str()
                .is_some_and(|t| t.contains("## Tool output gathered for you"))),
        "the rebuilt prompt carries the gathered output: {rebuilt}"
    );

    // A second refusal of the same capability is an ordinary failure: the attempt is not
    // rebuilt again and the next provider is tried. The first attempt's tool use is
    // forgotten with the rebuild; the second attempt's stays.
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.primary
        .push_ok(read_calls(&[("c1", "docker_ps", json!({}))]));
    w.primary.push_err(tools_refused());
    w.primary
        .push_ok(read_calls(&[("c2", "host_overview", json!({}))]));
    w.primary.push_err(tools_refused());
    w.secondary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    assert_eq!(
        w.primary.requests().len(),
        4,
        "two attempts, two requests each, no third attempt"
    );
    assert_eq!(w.secondary.requests().len(), 1, "the next provider answers");
    let tools = format!("{:?}", recorded_tools(&w).await);
    assert!(
        !tools.contains("docker_ps"),
        "the rebuilt attempt's call is forgotten: {tools}"
    );
    assert!(
        tools.contains("host_overview"),
        "the last attempt's call is kept: {tools}"
    );
}

/// Splits a failure text into its per-provider reasons.
fn parts(reason: &str) -> Vec<&str> {
    reason.split("; ").collect()
}

fn assert_reasons_in_order(reason: &str) {
    assert!(
        !reason.contains(SECRET),
        "a reason leaks a secret: {reason}"
    );
    let parts = parts(reason);
    assert_eq!(parts.len(), 3, "{reason}");
    for (part, label) in parts.iter().zip(["one", "two", "three"]) {
        assert!(
            part.starts_with("scripted failed: ") && part.contains(label),
            "{part}"
        );
    }
}

async fn three_failures(w: &World) {
    w.primary.push_err(down(&format!("one {SECRET}")));
    w.secondary.push_err(down(&format!("two {SECRET}")));
    w.third.push_err(down(&format!("three {SECRET}")));
}

#[tokio::test]
async fn every_failure_reason_is_returned_in_order_and_redacted() {
    let w = world_with_fallbacks().await;
    three_failures(&w).await;
    assert_reasons_in_order(&failed_reason(w.run().await));

    let w = world_with_fallbacks().await;
    three_failures(&w).await;
    assert_reasons_in_order(&provider_reason(follow(&w).await));

    let w = world_with_fallbacks().await;
    three_failures(&w).await;
    assert_reasons_in_order(&provider_reason(chatted(&w).await));
}

#[tokio::test]
async fn the_failure_texts_of_the_three_paths_are_unchanged() {
    let expected = "scripted failed: unavailable: first; scripted failed: unavailable: second";
    let two = || async {
        let w = world(Setup {
            secondary: true,
            ..Setup::default()
        })
        .await;
        w.primary.push_err(down("first"));
        w.secondary.push_err(down("second"));
        w
    };
    assert_eq!(failed_reason(two().await.run().await), expected);
    assert_eq!(provider_reason(follow(&two().await).await), expected);
    assert_eq!(provider_reason(chatted(&two().await).await), expected);

    // Follow-up and chat cut a long reason to 500 characters; triage keeps it whole.
    let long = || async {
        let w = world(Setup::default()).await;
        w.primary.push_err(down(&"y".repeat(2000)));
        w
    };
    for reason in [
        provider_reason(follow(&long().await).await),
        provider_reason(chatted(&long().await).await),
    ] {
        assert_eq!(reason.chars().count(), 500, "{reason}");
        assert!(reason.starts_with("scripted failed: unavailable: yyy"));
    }
    let whole = failed_reason(long().await.run().await);
    assert!(whole.chars().count() > 2000, "triage cut its reason");
}

// The same-output note, end to end.

/// A read answer long enough to be noted when repeated.
fn big() -> String {
    "a".repeat(8 * 1024)
}

fn serve(w: &World, text: String) {
    w.link.on_tool(move |_, _| {
        Ok(ToolOutput {
            text: text.clone(),
            truncated: false,
        })
    });
}

fn note(call: u32, at: impl Display) -> String {
    format!(
        "Same output as your earlier read (tool call {call}, taken {at}); nothing changed, so it is not repeated."
    )
}

fn overview_reads(w: &World) -> usize {
    w.link
        .requests()
        .iter()
        .filter(|r| {
            matches!(
                r,
                LinkRequest::Tool {
                    call: ReadToolCall::HostOverview,
                    ..
                }
            )
        })
        .count()
}

/// The model scripted to read the host overview in `turns` separate turns.
fn script_reads(w: &World, turns: usize) {
    for n in 0..turns {
        let id = format!("c{n}");
        w.primary
            .push_ok(read_calls(&[(&id, "host_overview", json!({}))]));
    }
}

/// The first tool result of `request`'s last message.
fn first_result(request: &LlmRequest) -> String {
    results(request).remove(0).0
}

/// The time named in a same-output note for tool call `call`.
fn noted_time(content: &str, call: u32) -> Timestamp {
    let head = format!("Same output as your earlier read (tool call {call}, taken ");
    let tail = "); nothing changed, so it is not repeated.";
    let at = content
        .strip_prefix(&head)
        .and_then(|rest| rest.strip_suffix(tail))
        .unwrap_or_else(|| panic!("not the note for call {call}: {content}"));
    at.parse()
        .unwrap_or_else(|_| panic!("not an RFC 3339 time: {at}"))
}

#[tokio::test]
async fn a_repeated_identical_read_reaches_the_link_but_the_model_gets_a_note() {
    let w = world(Setup::default()).await;
    serve(&w, big());
    script_reads(&w, 2);
    w.primary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    assert_eq!(overview_reads(&w), 2, "both reads reached the link");
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(first_result(&requests[1]), big(), "the first read is whole");
    // The run's clock moves while it works, so the first read's time is checked as a time.
    let taken = noted_time(&first_result(&requests[2]), 1);
    assert!(taken >= T0 && taken <= w.now(), "{taken}");
}

#[tokio::test]
async fn a_repeated_read_still_counts_against_the_tool_call_budget() {
    let mut settings = domovoi_engine::triage::TriageSettings::default();
    settings.budget.max_tool_calls = 2;
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    serve(&w, big());
    w.primary.push_ok(read_calls(&[
        ("c1", "host_overview", json!({})),
        ("c2", "host_overview", json!({})),
        ("c3", "host_overview", json!({})),
    ]));
    w.primary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    assert_eq!(overview_reads(&w), 2, "the third read never ran");
    let shown = results(&w.primary.requests()[1]);
    assert_eq!(shown.len(), 3);
    assert_eq!(shown[0].0, big());
    noted_time(&shown[1].0, 1);
    assert_eq!(shown[2].0, NO_CALLS_LEFT, "the repeat used up a call");
}

#[tokio::test]
async fn a_changed_read_is_returned_in_full() {
    let w = world(Setup::default()).await;
    let reads = Arc::new(AtomicUsize::new(0));
    let counter = reads.clone();
    w.link.on_tool(move |_, _| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput {
            text: format!("read {n}: {}", "a".repeat(4096)),
            truncated: false,
        })
    });
    script_reads(&w, 2);
    w.primary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    let requests = w.primary.requests();
    // The run's own opening reads use the counter first, so only the order is fixed.
    let tail = "a".repeat(4096);
    let first = first_result(&requests[1]);
    let second = first_result(&requests[2]);
    assert!(
        first.starts_with("read ") && first.ends_with(&tail),
        "{first}"
    );
    assert!(
        second.starts_with("read ") && second.ends_with(&tail),
        "{second}"
    );
    assert_ne!(first, second);
}

#[tokio::test]
async fn the_evidence_record_keeps_the_real_text() {
    use domovoi_core::ports::OperationRepo;
    let w = world(Setup::default()).await;
    serve(&w, big());
    script_reads(&w, 2);
    w.primary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    // The model was told it had seen the text before.
    noted_time(&first_result(&w.primary.requests()[2]), 1);

    let operation = OperationRepo::for_incident(&*w.store, w.incident.id())
        .await
        .unwrap()
        .remove(0);
    let snapshot = OperationRepo::load(&*w.store, operation.id)
        .await
        .unwrap()
        .unwrap();
    let outputs: Vec<String> = snapshot
        .steps
        .iter()
        .filter(|record| {
            record.step.kind == StepKind::EvidenceRequest
                && record.progress.outcome == StepOutcome::Succeeded
        })
        .map(|record| record.progress.output.clone().unwrap_or_default())
        .collect();
    assert_eq!(outputs.len(), 2, "{outputs:?}");
    assert_eq!(outputs[0], big());
    assert_eq!(outputs[1], big(), "the record holds the text, not the note");
}

#[tokio::test]
async fn follow_up_and_chat_dedupe_too() {
    let w = world(Setup::default()).await;
    serve(&w, big());
    script_reads(&w, 2);
    w.primary.push_text("Nothing changed.");
    assert_eq!(follow(&w).await.unwrap(), "Nothing changed.");
    assert_eq!(overview_reads(&w), 2);
    let requests = w.primary.requests();
    assert_eq!(first_result(&requests[1]), big());
    assert_eq!(
        first_result(&requests[2]),
        note(1, w.now()),
        "the follow-up's clock does not move, so the time is exact"
    );

    let w = world(Setup::default()).await;
    serve(&w, big());
    script_reads(&w, 2);
    w.primary.push_text("Still fine.");
    assert_eq!(chatted(&w).await.unwrap(), "Still fine.");
    assert_eq!(overview_reads(&w), 2);
    let requests = w.primary.requests();
    assert_eq!(first_result(&requests[1]), big());
    assert_eq!(first_result(&requests[2]), note(1, w.now()));
}

#[tokio::test]
async fn a_rebuilt_attempt_starts_with_an_empty_ledger() {
    let w = world(Setup::default()).await;
    serve(&w, big());
    w.primary
        .push_ok(read_calls(&[("c1", "host_overview", json!({}))]));
    w.primary.push_err(tools_refused());
    w.primary
        .push_ok(read_calls(&[("c2", "host_overview", json!({}))]));
    w.primary.push_text(report());
    let outcome = w.run().await;
    assert!(matches!(outcome, TriageOutcome::Report(_)), "{outcome:?}");
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_eq!(first_result(&requests[1]), big());
    assert_eq!(
        first_result(&requests[3]),
        big(),
        "the repeat after the rebuild is a first read again"
    );
}

#[tokio::test]
async fn short_output_repeated_is_returned_in_full() {
    let w = world(Setup::default()).await;
    serve(&w, "disk 92%".to_owned());
    script_reads(&w, 2);
    w.primary.push_text(report());
    w.run().await;
    let requests = w.primary.requests();
    assert_eq!(first_result(&requests[1]), "disk 92%");
    assert_eq!(first_result(&requests[2]), "disk 92%");
}
