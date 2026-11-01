use domovoi_core::ports::Usage;
use domovoi_llm::{Price, build, cost_micros};
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{anthropic, ask, openai};

const PRICE: Price = Price {
    input_per_mtok_usd: 3.0,
    output_per_mtok_usd: 15.0,
};

#[tokio::test]
async fn cost_is_computed_only_with_configured_prices() {
    let usage = Usage {
        input_tokens: 1000,
        output_tokens: 200,
        cost_micros: None,
    };
    assert_eq!(cost_micros(None, &usage), None);
    assert_eq!(cost_micros(Some(&PRICE), &usage), Some(6000));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1000, "output_tokens": 200}
        })))
        .mount(&server)
        .await;
    let unpriced = build(&anthropic(&server.uri()), None).unwrap();
    assert_eq!(
        unpriced.complete(ask("x")).await.unwrap().usage.cost_micros,
        None
    );
    let mut priced = anthropic(&server.uri());
    priced.price = Some(PRICE);
    let priced = build(&priced, None).unwrap();
    assert_eq!(
        priced.complete(ask("x")).await.unwrap().usage.cost_micros,
        Some(6000)
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1000, "completion_tokens": 200}
        })))
        .mount(&server)
        .await;
    let mut settings = openai(&server.uri(), false);
    settings.price = Some(PRICE);
    let response = build(&settings, None)
        .unwrap()
        .complete(ask("x"))
        .await
        .unwrap();
    assert_eq!(response.usage.cost_micros, Some(6000));
}
