//! Engine and real OpenAI-compatible adapter together: capability refusals and a
//! reasoning model that runs out of output room.

use std::sync::{Arc, OnceLock};

use domovoi_core::ports::{LlmProvider, UsageRepo};
use domovoi_core::{TriageBudget, TriageOutcome, UserId};
use domovoi_engine::triage::TriageSettings;
use domovoi_llm::{ProviderKind, ProviderSettings, build};
use jiff::tz::TimeZone;
use serde_json::{Value, json};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use crate::harness::{Mode, Setup, host_id, report, world};

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
    provider_with_price(base, None)
}

/// The same provider with a configured price, so a monthly budget can count it.
fn priced_provider(base: &str) -> Arc<dyn LlmProvider> {
    provider_with_price(
        base,
        Some(domovoi_llm::Price {
            input_per_mtok_usd: 1.0,
            output_per_mtok_usd: 2.0,
        }),
    )
}

fn provider_with_price(base: &str, price: Option<domovoi_llm::Price>) -> Arc<dyn LlmProvider> {
    crypto();
    let mut settings = ProviderSettings::new(ProviderKind::OpenaiCompatible);
    settings.base_url = Some(Url::parse(&format!("{base}/v1")).unwrap());
    settings.model = Some("model-under-test".to_owned());
    settings.price = price;
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
    let outcome = w.run(Mode::Legacy).await;
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
    let outcome = w.run(Mode::Legacy).await;
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
    let outcome = w.run(Mode::Legacy).await;
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
    let outcome = w.run(Mode::Legacy).await;
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

/// A tools rejection is definite and generated nothing, so the rebuild in the same run gets
/// the run's whole budget back: a tight cumulative budget still allows the no-tools answer.
#[tokio::test]
async fn a_refused_tools_rejection_refunds_the_run_budget() {
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
                    }],
                    "usage": {"prompt_tokens": 100, "completion_tokens": 50}
                }))
            }
        })
        .mount(&server)
        .await;

    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 15,
            max_seconds: 90,
            max_output_tokens: 2000,
            max_total_output_tokens: 2000,
        },
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        provider: Some(provider(&server.uri())),
        ..Setup::default()
    })
    .await;
    let outcome = w.run(Mode::Legacy).await;
    assert!(
        matches!(outcome, TriageOutcome::Report(_)),
        "the refund lets the rebuilt attempt use the allowance: {outcome:?}"
    );

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let rebuilt = body_of(&requests[1]);
    assert!(rebuilt.get("tools").is_none());
    assert_eq!(
        rebuilt["max_tokens"], 2000,
        "the rejected request's allowance was refunded exactly once"
    );
}

/// An uncountable answer from a priced provider is still billed — input and a conservative
/// output bound — and the fallback's answer is recorded too. The charge is visible to the
/// monthly budget.
#[tokio::test]
async fn an_uncountable_answer_is_billed_and_the_fallback_is_recorded_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {"role": "assistant", "content": ""},
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;

    let settings = TriageSettings {
        monthly_budget_micros: Some(1_000),
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        provider: Some(priced_provider(&server.uri())),
        secondary: true,
        ..Setup::default()
    })
    .await;
    w.secondary.push_text(report());

    let outcome = w.run(Mode::Legacy).await;
    assert!(
        matches!(outcome, TriageOutcome::Report(_)),
        "the priced fallback answers: {outcome:?}"
    );
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests.len(),
        2,
        "the unusable answer and its repair are both billed"
    );
    let month = w.now().to_zoned(TimeZone::UTC).date();
    let total = w.store.month_total(month).await.expect("usage");
    assert_eq!(
        total.tokens_out, 4000,
        "two 2000-token conservative bounds, not lost or doubled"
    );
    assert!(total.tokens_in > 0, "the input cost is retained");
    assert!(total.cost_micros > 0, "the attempts are not free");

    let again = w.run(Mode::Legacy).await;
    assert!(
        matches!(&again, TriageOutcome::Skipped { reason } if reason.contains("monthly")),
        "the conservative charge counts against the monthly budget: {again:?}"
    );
}

/// A chat whose chosen provider refuses tools is rebuilt the same way triage is: the host's
/// read output goes into the prompt and the answer is accepted without tool calling.
#[tokio::test]
async fn a_chat_provider_that_refuses_tools_is_rebuilt_with_host_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|request: &Request| {
            if body_of(request).get("tools").is_some() {
                ResponseTemplate::new(400).set_body_string("tools are not supported by this model")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {"role": "assistant", "content": "The disk is full."},
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 5}
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
    let answer = w
        .triage
        .chat(Some(host_id()), "One host.", &Vec::new(), "Why is it slow?")
        .await
        .expect("the rebuilt answer is used");
    assert_eq!(answer, "The disk is full.".to_owned());

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "one refusal, one rebuilt answer");
    let rebuilt = body_of(&requests[1]);
    assert!(rebuilt.get("tools").is_none());
    let messages = rebuilt["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|message| message["content"]
            .as_str()
            .is_some_and(|text| text.contains("## Tool output gathered for you"))),
        "the rebuilt chat carries the host's read output: {messages:?}"
    );
}

/// A follow-up question whose provider refuses tools is rebuilt without them and still
/// answers from the gathered host output.
#[tokio::test]
async fn a_follow_up_provider_that_refuses_tools_is_rebuilt_with_host_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(|request: &Request| {
            if body_of(request).get("tools").is_some() {
                ResponseTemplate::new(400).set_body_string("tools are not supported by this model")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "choices": [{
                        "message": {"role": "assistant", "content": "The container is restarting."},
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 10, "completion_tokens": 5}
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
    let answer = w
        .triage
        .follow_up(w.incident.id(), "Why does it restart?", UserId::new())
        .await
        .expect("the rebuilt answer is used");
    assert_eq!(answer, "The container is restarting.".to_owned());

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2, "one refusal, one rebuilt answer");
    let rebuilt = body_of(&requests[1]);
    assert!(rebuilt.get("tools").is_none());
    let messages = rebuilt["messages"].as_array().unwrap();
    assert!(
        messages.iter().any(|message| message["content"]
            .as_str()
            .is_some_and(|text| text.contains("## Tool output gathered for you"))),
        "the rebuilt follow-up carries the host's read output: {messages:?}"
    );
}
