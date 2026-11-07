use std::time::Duration;

use domovoi_core::ports::{LlmError, Part, StopReason};
use domovoi_llm::build;
use jiff::SignedDuration;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{anthropic, ask, openai};

fn events(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_owned(), "text/event-stream")
}

async fn serve(route: &str, response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(route))
        .respond_with(response)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn openai_stream_assembles_text_and_usage() {
    let server = serve(
        "/v1/chat/completions",
        events(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"All \"}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"good.\"},\"finish_reason\":\"stop\"}]}\n\n\
             data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\n\
             data: [DONE]\n\n",
        ),
    )
    .await;
    let provider = build(&openai(&server.uri(), false), None).unwrap();
    let response = provider.complete(ask("Status?")).await.unwrap();
    assert_eq!(response.parts, vec![Part::Text("All good.".to_owned())]);
    assert_eq!(response.stop, StopReason::EndTurn);
    assert_eq!(response.usage.input_tokens, 7);
    assert_eq!(response.usage.output_tokens, 3);

    let received = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
}

#[tokio::test]
async fn openai_stream_joins_tool_call_fragments() {
    let server = serve(
        "/v1/chat/completions",
        events(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"docker_ps\",\"arguments\":\"\"}}]}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"all\\\":\"}}]}}]}\n\n\
             data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" true}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
             data: [DONE]\n\n",
        ),
    )
    .await;
    let provider = build(&openai(&server.uri(), false), None).unwrap();
    let response = provider.complete(ask("Which are down?")).await.unwrap();
    assert_eq!(response.stop, StopReason::ToolUse);
    assert_eq!(
        response.parts,
        vec![Part::ToolCall {
            id: "call_1".to_owned(),
            name: "docker_ps".to_owned(),
            input: json!({"all": true}),
        }]
    );
}

#[tokio::test]
async fn openai_stream_cut_off_before_the_end_is_unavailable() {
    let server = serve(
        "/v1/chat/completions",
        events("data: {\"choices\":[{\"delta\":{\"content\":\"All \"}}]}\n\n"),
    )
    .await;
    let provider = build(&openai(&server.uri(), false), None).unwrap();
    let err = provider.complete(ask("Status?")).await.unwrap_err();
    assert!(matches!(err, LlmError::Unavailable(_)), "{err:?}");
}

#[tokio::test]
async fn a_silent_server_times_out_once_the_idle_limit_passes() {
    let server = serve(
        "/v1/chat/completions",
        events("data: [DONE]\n\n").set_delay(Duration::from_millis(1500)),
    )
    .await;
    let mut settings = openai(&server.uri(), false);
    settings.timeout = SignedDuration::from_millis(300);
    let provider = build(&settings, None).unwrap();
    let err = provider.complete(ask("Status?")).await.unwrap_err();
    assert!(
        matches!(&err, LlmError::Unavailable(m) if m.contains("timed out")),
        "{err:?}"
    );
}

#[tokio::test]
async fn anthropic_stream_assembles_text_tool_use_and_usage() {
    let server = serve(
        "/v1/messages",
        events(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n\
             event: content_block_start\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             event: ping\n\
             data: {\"type\":\"ping\"}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" now.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"docker_ps\",\"input\":{}}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"all\\\":\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\" true}\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":42}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&anthropic(&server.uri()), None).unwrap();
    let response = provider.complete(ask("Which are down?")).await.unwrap();
    assert_eq!(response.stop, StopReason::ToolUse);
    assert_eq!(
        response.parts,
        vec![
            Part::Text("Checking now.".to_owned()),
            Part::ToolCall {
                id: "toolu_1".to_owned(),
                name: "docker_ps".to_owned(),
                input: json!({"all": true}),
            },
        ]
    );
    assert_eq!(response.usage.input_tokens, 25);
    assert_eq!(response.usage.output_tokens, 42);

    let received = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn anthropic_stream_error_event_maps_to_an_llm_error() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n\
             data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        ),
    )
    .await;
    let provider = build(&anthropic(&server.uri()), None).unwrap();
    let err = provider.complete(ask("hello")).await.unwrap_err();
    assert!(matches!(err, LlmError::Unavailable(_)), "{err:?}");
}

#[tokio::test]
async fn anthropic_stream_cut_off_before_message_stop_is_unavailable() {
    let server = serve(
        "/v1/messages",
        events("data: {\"type\":\"message_start\",\"message\":{}}\n\n"),
    )
    .await;
    let provider = build(&anthropic(&server.uri()), None).unwrap();
    let err = provider.complete(ask("hello")).await.unwrap_err();
    assert!(matches!(err, LlmError::Unavailable(_)), "{err:?}");
}
