use domovoi_core::ports::{LlmMessage, Part, Role, StopReason, ToolSpec};
use domovoi_llm::build;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{KEY, anthropic, ask, user};

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
