use std::time::Duration;

use domovoi_core::ports::{LlmError, Part, StopReason, UsageSource};
use domovoi_llm::{Price, build};
use jiff::SignedDuration;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{anthropic, ask, openai};

const ANTHROPIC_PRICE: Price = Price {
    input_per_mtok_usd: 2.0,
    output_per_mtok_usd: 10.0,
};

fn priced_anthropic(base: &str) -> domovoi_llm::ProviderSettings {
    let mut settings = anthropic(base);
    settings.price = Some(ANTHROPIC_PRICE);
    settings
}

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

/// Thinking deltas arrive as `thinking` blocks; they are counted for usage and never become
/// answer text.
#[tokio::test]
async fn anthropic_stream_thinking_is_estimated_and_never_visible() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason reason \"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"All good.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let response = provider.complete(ask("Which are down?")).await.unwrap();
    assert_eq!(
        response.parts,
        vec![Part::Text("All good.".to_owned())],
        "thinking is never answer text"
    );
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(
        response.usage.output_tokens >= 100,
        "the thinking deltas are counted: {:?}",
        response.usage
    );
    assert!(response.usage.cost_micros.is_some_and(|cost| cost > 0));
}

/// A stream that reported input usage but never reported output usage keeps the input total
/// and estimates only the output.
#[tokio::test]
async fn anthropic_stream_input_usage_without_output_is_estimated() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking now.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let response = provider.complete(ask("Which are down?")).await.unwrap();
    assert_eq!(
        response.usage.input_tokens, 25,
        "the reported input is kept"
    );
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(
        response.usage.output_tokens > 0,
        "the output is estimated: {:?}",
        response.usage
    );
    assert_eq!(
        response.usage.cost_micros,
        Some(25 * 2 + u64::from(response.usage.output_tokens) * 10)
    );
}

/// Both halves reported is complete usage: it wins over the thinking text.
#[tokio::test]
async fn anthropic_stream_complete_usage_wins_over_thinking() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"a long private chain\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"All good.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let response = provider.complete(ask("Which are down?")).await.unwrap();
    assert_eq!(response.usage.source, UsageSource::Reported);
    assert_eq!(response.usage.input_tokens, 25);
    assert_eq!(response.usage.output_tokens, 42, "not doubled by thinking");
    assert_eq!(response.usage.cost_micros, Some(25 * 2 + 42 * 10));
}

/// A stream that ran out of room and reported no usage is estimated at the whole allowance.
#[tokio::test]
async fn anthropic_stream_max_tokens_without_usage_estimates_the_allowance() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial answer\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let mut req = ask("Which are down?");
    req.max_output_tokens = 2000;
    let response = provider.complete(req).await.unwrap();
    assert_eq!(response.stop, StopReason::MaxTokens);
    assert_eq!(response.usage.output_tokens, 2000);
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(response.usage.cost_micros.is_some_and(|cost| cost > 0));
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

/// A redacted-thinking block has no text to count: without output usage the stream is charged
/// its whole allowance, and the encrypted payload never becomes answer text.
#[tokio::test]
async fn anthropic_stream_redacted_thinking_charges_the_allowance() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"emr_this_payload_is_private\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"All good.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let mut req = ask("Which are down?");
    req.max_output_tokens = 2000;
    let response = provider.complete(req).await.unwrap();
    assert_eq!(
        response.parts,
        vec![Part::Text("All good.".to_owned())],
        "the redacted payload never reaches the answer"
    );
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert_eq!(response.usage.output_tokens, 2000);
    assert!(response.usage.cost_micros.is_some_and(|cost| cost > 0));
}

/// A stream that reported its input but not its output keeps the input and charges the
/// allowance for the redacted reasoning.
#[tokio::test]
async fn anthropic_stream_redacted_with_input_usage_keeps_input() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"emr_private\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"All good.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let mut req = ask("Which are down?");
    req.max_output_tokens = 2000;
    let response = provider.complete(req).await.unwrap();
    assert_eq!(
        response.usage.input_tokens, 25,
        "the reported input is kept"
    );
    assert_eq!(response.usage.output_tokens, 2000);
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert_eq!(response.usage.cost_micros, Some(25 * 2 + 2000 * 10));
}

/// Output usage reported without an input total stays authoritative; only the input is
/// estimated.
#[tokio::test]
async fn anthropic_stream_output_only_usage_stays_authoritative() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"emr_private\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"All good.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let mut req = ask("Which are down?");
    req.max_output_tokens = 2000;
    let response = provider.complete(req).await.unwrap();
    assert_eq!(response.usage.output_tokens, 42, "the reported output wins");
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(
        response.usage.input_tokens > 0,
        "the input is estimated: {:?}",
        response.usage
    );
    assert_eq!(
        response.usage.cost_micros,
        Some(u64::from(response.usage.input_tokens) * 2 + 42 * 10)
    );
}

/// Complete reported totals with redacted reasoning remain authoritative and are not
/// replaced by the allowance.
#[tokio::test]
async fn anthropic_stream_complete_usage_with_redacted_wins() {
    let server = serve(
        "/v1/messages",
        events(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"emr_private\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
             data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
             data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"All good.\"}}\n\n\
             data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}\n\n\
             data: {\"type\":\"message_stop\"}\n\n",
        ),
    )
    .await;
    let provider = build(&priced_anthropic(&server.uri()), None).unwrap();
    let mut req = ask("Which are down?");
    req.max_output_tokens = 2000;
    let response = provider.complete(req).await.unwrap();
    assert_eq!(response.usage.source, UsageSource::Reported);
    assert_eq!(response.usage.input_tokens, 25);
    assert_eq!(
        response.usage.output_tokens, 42,
        "not replaced by the allowance"
    );
    assert_eq!(response.usage.cost_micros, Some(25 * 2 + 42 * 10));
}
