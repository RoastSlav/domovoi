use domovoi_core::ports::{LlmMessage, Part, Role, StopReason, ToolSpec, UsageSource};
use domovoi_llm::{Price, build};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{KEY, anthropic, ask, user};

const PRICE: Price = Price {
    input_per_mtok_usd: 2.0,
    output_per_mtok_usd: 10.0,
};

fn priced(base: &str) -> domovoi_llm::ProviderSettings {
    let mut settings = anthropic(base);
    settings.price = Some(PRICE);
    settings
}

#[tokio::test]
async fn anthropic_text_response_maps_to_parts_and_usage() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", KEY))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "…"},
                {"type": "text", "text": "The disk is full."}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 120, "output_tokens": 9}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let provider = build(&anthropic(&server.uri()), None).unwrap();
    assert_eq!(provider.id(), "anthropic");
    assert!(provider.supports_tools());
    let response = provider.complete(ask("Why is nas down?")).await.unwrap();
    assert_eq!(
        response.parts,
        vec![Part::Text("The disk is full.".to_owned())]
    );
    assert_eq!(response.stop, StopReason::EndTurn);
    assert_eq!(response.usage.input_tokens, 120);
    assert_eq!(response.usage.output_tokens, 9);
    assert_eq!(response.usage.cost_micros, None);
}

#[tokio::test]
async fn anthropic_tool_use_round_trip_request_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [
                {"type": "text", "text": "Checking the journal."},
                {"type": "tool_use", "id": "toolu_2", "name": "journal", "input": {"unit": "nginx"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 300, "output_tokens": 40}
        })))
        .mount(&server)
        .await;

    let mut req = ask("Why is nginx failing?");
    req.tools = vec![ToolSpec {
        name: "journal".to_owned(),
        description: "Recent journal lines for a unit.".to_owned(),
        input_schema: json!({"type": "object", "properties": {"unit": {"type": "string"}}}),
    }];
    req.messages.push(LlmMessage {
        role: Role::Assistant,
        parts: vec![
            Part::Text("Let me look at the host.".to_owned()),
            Part::ToolCall {
                id: "toolu_1".to_owned(),
                name: "host_overview".to_owned(),
                input: json!({}),
            },
        ],
    });
    req.messages.push(LlmMessage {
        role: Role::User,
        parts: vec![Part::ToolResult {
            id: "toolu_1".to_owned(),
            content: "load 0.4, disk 61%".to_owned(),
            is_error: false,
        }],
    });
    req.messages.push(user("Keep going."));

    let provider = build(&anthropic(&server.uri()), None).unwrap();
    let response = provider.complete(req).await.unwrap();
    assert_eq!(response.stop, StopReason::ToolUse);
    assert_eq!(
        response.parts[1],
        Part::ToolCall {
            id: "toolu_2".to_owned(),
            name: "journal".to_owned(),
            input: json!({"unit": "nginx"}),
        }
    );

    let received = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    insta::assert_json_snapshot!("anthropic_tool_round_trip_request", body);
}

/// Thinking is billed output but private: it counts toward usage and never appears in the
/// answer's parts.
#[tokio::test]
async fn anthropic_thinking_is_counted_but_never_returned() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [
                {"type": "thinking", "thinking": "reason ".repeat(80)},
                {"type": "text", "text": "The disk is full."}
            ],
            "stop_reason": "end_turn"
        })))
        .mount(&server)
        .await;

    let provider = build(&priced(&server.uri()), None).unwrap();
    let response = provider.complete(ask("Why is nas down?")).await.unwrap();
    assert_eq!(
        response.parts,
        vec![Part::Text("The disk is full.".to_owned())],
        "thinking is not answer text"
    );
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(
        response.usage.output_tokens >= 100,
        "the thinking is counted: {:?}",
        response.usage
    );
    assert!(
        response.usage.cost_micros.is_some_and(|cost| cost > 0),
        "thinking is billed: {:?}",
        response.usage
    );
}

/// A whole response that ran out of room with no usage is estimated at its whole allowance.
#[tokio::test]
async fn anthropic_max_tokens_without_usage_estimates_the_allowance() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "The disk is"}],
            "stop_reason": "max_tokens"
        })))
        .mount(&server)
        .await;

    let provider = build(&priced(&server.uri()), None).unwrap();
    let mut req = ask("Why is nas down?");
    req.max_output_tokens = 2000;
    let response = provider.complete(req).await.unwrap();
    assert_eq!(response.stop, StopReason::MaxTokens);
    assert_eq!(response.usage.output_tokens, 2000);
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(response.usage.cost_micros.is_some_and(|cost| cost > 0));
}

/// Complete reported totals already include thinking tokens: they win, and are priced once.
#[tokio::test]
async fn anthropic_reported_usage_wins_over_thinking() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [
                {"type": "thinking", "thinking": "a long private chain of thought"},
                {"type": "text", "text": "The disk is full."}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 120, "output_tokens": 9}
        })))
        .mount(&server)
        .await;

    let provider = build(&priced(&server.uri()), None).unwrap();
    let response = provider.complete(ask("Why is nas down?")).await.unwrap();
    assert_eq!(response.usage.source, UsageSource::Reported);
    assert_eq!(response.usage.input_tokens, 120);
    assert_eq!(response.usage.output_tokens, 9, "not doubled by thinking");
    assert_eq!(response.usage.cost_micros, Some(120 * 2 + 9 * 10));
}
