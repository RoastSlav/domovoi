//! Engine and real OpenAI-compatible adapter together: capability refusals and a
//! reasoning model that runs out of output room.

use std::sync::{Arc, OnceLock};

use domovoi_core::TriageOutcome;
use domovoi_core::ports::LlmProvider;
use domovoi_llm::{ProviderKind, ProviderSettings, build};
use serde_json::{Value, json};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::harness::{Setup, report, world};

/// Installs the rustls provider once, as the binary does at startup.
fn crypto() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// A real OpenAI-compatible provider pointed at `base`, so the test covers the adapter and
/// the engine together rather than a scripted port.
fn provider(base: &str) -> Arc<dyn LlmProvider> {
    crypto();
    let mut settings = ProviderSettings::new(ProviderKind::OpenaiCompatible);
    settings.base_url = Some(Url::parse(&format!("{base}/v1")).unwrap());
    settings.model = Some("model-under-test".to_owned());
    build(&settings, None).unwrap()
}

fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

/// One server-sent event carrying `chunk`, then the end marker.
fn sse(chunks: &[Value]) -> String {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    body
}

fn streamed(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

/// A server can refuse two capabilities in a row: the engine retries the request without
/// streamed usage, then rebuilds the run without tools and prefetches their output. Nothing
/// is negotiated twice.
#[tokio::test]
async fn stream_usage_then_tools_are_both_handled_without_a_loop() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|request: &Request| {
            let body = body_of(request);
            if body.get("stream_options").is_some() {
                ResponseTemplate::new(400).set_body_string("unknown field: stream_options")
            } else if body.get("tools").is_some() {
                ResponseTemplate::new(400).set_body_string("tools are not supported by this model")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {"role": "assistant", "content": report()},
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 100, "completion_tokens": 50}
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
    assert_eq!(
        requests.len(),
        3,
        "one refusal per capability, then the answer, with no repeated negotiation"
    );
    let first = body_of(&requests[0]);
    assert!(first.get("stream_options").is_some());
    assert!(first.get("tools").is_some());
    let second = body_of(&requests[1]);
    assert!(
        second.get("stream_options").is_none(),
        "the refused usage option is gone"
    );
    assert_eq!(second["stream"], false, "the request is whole, for usage");
    assert!(
        second.get("tools").is_some(),
        "the tool capability is still offered after the usage refusal"
    );
    let rebuilt = body_of(&requests[2]);
    assert!(rebuilt.get("tools").is_none());
    assert!(rebuilt.get("stream_options").is_none());
    let opening = rebuilt["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default();
    assert!(
        opening.contains("## Tool output gathered for you"),
        "the rebuilt run carries the prefetched evidence: {opening}"
    );
}

/// A provider that refuses tools is rebuilt within the same run, and the rebuilt prompt
/// carries the tool output the model can no longer gather itself.
#[tokio::test]
async fn a_provider_that_refuses_tools_gets_prefetched_output_in_the_same_run() {
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
    assert_eq!(
        requests.len(),
        2,
        "one attempt refused the tools, one rebuilt without them"
    );
    let first = body_of(&requests[0]);
    let rebuilt = body_of(&requests[1]);
    assert!(first.get("tools").is_some());
    assert!(rebuilt.get("tools").is_none());
    let opening = rebuilt["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "user")
        .and_then(|m| m["content"].as_str())
        .unwrap_or_default();
    assert!(
        opening.contains("## Tool output gathered for you"),
        "the rebuilt prompt must carry the output it can no longer fetch: {opening}"
    );
    assert!(
        opening.contains("You cannot call tools"),
        "the model is told why the output is in front of it"
    );
}

/// A reasoning model that spends its whole allowance thinking, as GLM 5.3 Flash does,
/// answers after the allowance grows. The second request carries the larger cap.
#[tokio::test]
async fn a_reasoning_model_that_runs_out_is_retried_and_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|request: &Request| {
            let body = body_of(request);
            let max_tokens = body["max_tokens"].as_u64().unwrap_or_default();
            if max_tokens <= 2000 {
                streamed(sse(&[
                    json!({"choices": [{"index": 0, "delta": {"reasoning": "weighing the evidence"}, "finish_reason": null}]}),
                    json!({"choices": [{"index": 0, "delta": {"content": ""}, "finish_reason": "length"}]}),
                ]))
            } else {
                streamed(sse(&[
                    json!({"choices": [{"index": 0, "delta": {"content": report()}, "finish_reason": "stop"}]}),
                ]))
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
    assert_eq!(requests.len(), 2, "one cut-off turn, one answer");
    assert_eq!(body_of(&requests[0])["max_tokens"], 2000);
    let rebuilt = body_of(&requests[1]);
    assert_eq!(
        rebuilt["max_tokens"], 8000,
        "the retry asks for four times the configured allowance"
    );
    let messages = rebuilt["messages"].as_array().unwrap().clone();
    assert_eq!(
        messages.len(),
        2,
        "the cut-off turn is not kept before the retry"
    );
}

/// A provider that supports tools keeps working as before: the model calls one, the engine
/// runs it and sends the result back, and the next turn carries the report.
#[tokio::test]
async fn a_tool_calling_provider_still_runs_its_call_and_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|request: &Request| {
            let body = body_of(request);
            let has_tool_result = body["messages"]
                .as_array()
                .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
            if has_tool_result {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {"role": "assistant", "content": report()},
                        "finish_reason": "stop"
                    }]
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{
                                "id": "call_1",
                                "type": "function",
                                "function": {"name": "docker_ps", "arguments": "{}"}
                            }]
                        },
                        "finish_reason": "tool_calls"
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
    assert_eq!(requests.len(), 2);
    let first = body_of(&requests[0]);
    assert!(first.get("tools").is_some());
    let second = body_of(&requests[1]);
    let messages = second["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|m| m["role"] == "tool"),
        "the tool result goes back to the model"
    );
}
