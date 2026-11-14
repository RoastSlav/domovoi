//! The hard per-call deadline wrapper.

use std::sync::Arc;

use domovoi_core::ports::{LlmError, LlmRequest, Usage};
use domovoi_llm::{DEFAULT_CALL_DEADLINE, with_deadline};
use domovoi_testkit::{FakeClock, ScriptedLlm};
use jiff::{SignedDuration, Timestamp};

fn request() -> LlmRequest {
    LlmRequest {
        system: "system".to_owned(),
        messages: Vec::new(),
        tools: Vec::new(),
        max_output_tokens: 100,
    }
}

fn clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(Timestamp::constant(1_700_000_000, 0)))
}

fn completion() -> Usage {
    Usage::default()
}

#[tokio::test]
async fn call_deadline_enforced() {
    let clock = clock();
    let llm = Arc::new(ScriptedLlm::new());
    llm.push_ok(domovoi_core::ports::LlmResponse {
        parts: vec![domovoi_core::ports::Part::Text("too late".to_owned())],
        stop: domovoi_core::ports::StopReason::EndTurn,
        usage: completion(),
    });
    llm.hold();
    let wrapped = with_deadline(llm.clone(), SignedDuration::from_secs(30), clock.clone());
    let handle = tokio::spawn(async move { wrapped.complete(request()).await });

    llm.wait_for_request().await;
    tokio::task::yield_now().await;
    assert!(!handle.is_finished(), "answered before the deadline passed");

    clock.advance(SignedDuration::from_secs(31));
    assert_eq!(
        handle.await.unwrap(),
        Err(LlmError::Timeout {
            after: SignedDuration::from_secs(30)
        }),
    );
}

#[tokio::test]
async fn call_inside_deadline_returns_the_answer() {
    let clock = clock();
    let llm = Arc::new(ScriptedLlm::new().with_model("test-model"));
    llm.push_text("in time");
    let wrapped = with_deadline(llm.clone(), DEFAULT_CALL_DEADLINE, clock.clone());

    assert_eq!(wrapped.id(), "scripted");
    assert_eq!(wrapped.model(), Some("test-model"));
    assert!(wrapped.supports_tools());
    assert!(wrapped.prices_usage());

    let response = wrapped.complete(request()).await.unwrap();
    assert_eq!(
        response.parts,
        vec![domovoi_core::ports::Part::Text("in time".to_owned())]
    );
}
