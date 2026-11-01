use domovoi_core::ports::{LlmError, LlmMessage, Part, Role, StopReason};
use domovoi_llm::build;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{KEY, ask, openai};

fn tool_call_response(arguments: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-1",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "docker_ps", "arguments": arguments}
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 80, "completion_tokens": 12, "total_tokens": 92}
    }))
}

#[tokio::test]
async fn openai_tool_calls_parse_arguments_and_reject_invalid_json() {
    let good = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(tool_call_response(r#"{"all": true}"#))
        .mount(&good)
        .await;

    let mut req = ask("Which containers are down?");
    req.messages.push(LlmMessage {
        role: Role::Assistant,
        parts: vec![Part::ToolCall {
            id: "call_0".to_owned(),
            name: "host_overview".to_owned(),
            input: json!({"verbose": false}),
        }],
    });
    req.messages.push(LlmMessage {
        role: Role::User,
        parts: vec![Part::ToolResult {
            id: "call_0".to_owned(),
            content: "agent offline".to_owned(),
            is_error: true,
        }],
    });
    let provider = build(&openai(&good.uri(), true), None).unwrap();
    assert_eq!(provider.id(), "openai_compatible");
    let response = provider.complete(req).await.unwrap();
    assert_eq!(response.stop, StopReason::ToolUse);
    assert_eq!(
        response.parts,
        vec![Part::ToolCall {
            id: "call_1".to_owned(),
            name: "docker_ps".to_owned(),
            input: json!({"all": true}),
        }]
    );
    assert_eq!(response.usage.input_tokens, 80);
    assert_eq!(response.usage.output_tokens, 12);

    let received = good.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(
        messages[2]["tool_calls"][0]["function"]["arguments"], r#"{"verbose":false}"#,
        "arguments go out as a JSON string"
    );
    assert_eq!(messages[3]["role"], "tool");
    assert_eq!(messages[3]["tool_call_id"], "call_0");
    assert_eq!(messages[3]["content"], "error: agent offline");

    let bad = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(tool_call_response(r#"{"all": tru"#))
        .mount(&bad)
        .await;
    let provider = build(&openai(&bad.uri(), true), None).unwrap();
    let err = provider.complete(ask("again")).await.unwrap_err();
    assert!(
        matches!(&err, LlmError::InvalidResponse(m) if m.contains("docker_ps")),
        "{err:?}"
    );
}

#[tokio::test]
async fn openai_works_without_api_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {"role": "assistant", "content": "All good."},
                "finish_reason": "length"
            }]
        })))
        .mount(&server)
        .await;

    let provider = build(&openai(&server.uri(), false), None).unwrap();
    let response = provider.complete(ask("Status?")).await.unwrap();
    assert_eq!(response.parts, vec![Part::Text("All good.".to_owned())]);
    assert_eq!(response.stop, StopReason::MaxTokens);
    assert_eq!(response.usage.input_tokens, 0, "usage is optional");

    let received = server.received_requests().await.unwrap();
    assert!(received[0].headers.get("authorization").is_none());
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(body["model"], "llama3");
    assert!(body.get("tools").is_none());
}
