use domovoi_core::ports::{LlmCapability, LlmError, LlmMessage, Part, Role, StopReason, ToolSpec};
use domovoi_llm::build;
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

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

/// The body of every request `server` received, oldest first.
async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

fn body_of(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

fn tools_request(text: &str) -> domovoi_core::ports::LlmRequest {
    let mut req = ask(text);
    req.tools = vec![ToolSpec {
        name: "host_overview".to_owned(),
        description: "Summarise the host.".to_owned(),
        input_schema: json!({"type": "object", "properties": {}}),
    }];
    req
}

fn answer(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "choices": [{
            "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"
        }]
    }))
}

/// A server that refuses `stream_options`: the adapter reports the capability once, stops
/// sending the field, and the next call works.
#[tokio::test]
async fn a_refused_stream_usage_capability_is_reported_and_dropped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &Request| {
            if body_of(request).get("stream_options").is_some() {
                ResponseTemplate::new(400).set_body_string("unknown field: stream_options")
            } else {
                answer("All good.")
            }
        })
        .mount(&server)
        .await;

    let provider = build(&openai(&server.uri(), false), None).unwrap();
    let err = provider.complete(ask("Status?")).await.unwrap_err();
    assert!(
        matches!(
            &err,
            LlmError::CapabilityRejected {
                capability: LlmCapability::StreamUsage,
                ..
            }
        ),
        "{err:?}"
    );

    let response = provider.complete(ask("Status?")).await.unwrap();
    assert_eq!(response.parts, vec![Part::Text("All good.".to_owned())]);

    let bodies = bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0]["stream_options"]["include_usage"], true);
    assert!(
        bodies[1].get("stream_options").is_none(),
        "the refused field is not sent again"
    );
    assert_eq!(bodies[1]["stream"], true, "the answer is still streamed");
}

/// A server that refuses tools: the adapter reports it and `supports_tools` turns false, so
/// the caller builds the next request without tools.
#[tokio::test]
async fn a_refused_tool_capability_is_reported_and_kept_off() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|request: &Request| {
            if body_of(request).get("tools").is_some() {
                ResponseTemplate::new(400).set_body_string("tools are not supported by this model")
            } else {
                answer("All good.")
            }
        })
        .mount(&server)
        .await;

    let provider = build(&openai(&server.uri(), false), None).unwrap();
    assert!(provider.supports_tools());
    let err = provider
        .complete(tools_request("Status?"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            &err,
            LlmError::CapabilityRejected {
                capability: LlmCapability::Tools,
                ..
            }
        ),
        "{err:?}"
    );
    assert!(
        !provider.supports_tools(),
        "the refusal is remembered for later calls"
    );

    let response = provider.complete(tools_request("Status?")).await.unwrap();
    assert_eq!(response.parts, vec![Part::Text("All good.".to_owned())]);

    let bodies = bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    assert!(bodies[0].get("tools").is_some());
    assert!(
        bodies[1].get("tools").is_none(),
        "a later request does not offer tools the server refused"
    );
}

/// A 400 about anything else stays an ordinary error and changes no capability. An invalid
/// model, a context overflow, a bad tool schema, bad parameters and a bad history are not
/// evidence that tools or streamed usage are unsupported.
#[tokio::test]
async fn unrelated_400_errors_do_not_drop_capabilities() {
    for detail in [
        "model gpt-nope does not exist",
        "this model's maximum context length is 8192 tokens",
        "tools[0].function.parameters is invalid",
        "temperature must be between 0 and 2",
        "messages[1] role is invalid",
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string(detail))
            .mount(&server)
            .await;
        let provider = build(&openai(&server.uri(), false), None).unwrap();

        let err = provider
            .complete(tools_request("Status?"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, LlmError::InvalidResponse(m) if m.contains("HTTP 400")),
            "{detail}: {err:?}"
        );
        assert!(provider.supports_tools(), "{detail}: tools stay available");

        let err = provider
            .complete(tools_request("Again?"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, LlmError::InvalidResponse(_)),
            "{detail}: {err:?}"
        );
        let bodies = bodies(&server).await;
        assert!(
            bodies[1].get("stream_options").is_some(),
            "{detail}: streamed usage stays available"
        );
        assert!(
            bodies[1].get("tools").is_some(),
            "{detail}: tools stay in the request"
        );
    }
}
