use domovoi_llm::list_models;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{KEY, anthropic, openai};

#[tokio::test]
async fn openai_model_discovery_uses_the_base_url_and_sorts_ids() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "z-model"}, {"id": "a-model"}, {"id": "z-model"}]
        })))
        .mount(&server)
        .await;

    let models = list_models(&openai(&server.uri(), true)).await.unwrap();
    assert_eq!(models, ["a-model", "z-model"]);
}

#[tokio::test]
async fn anthropic_model_discovery_sends_the_api_key_and_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", KEY))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "claude-test"}]
        })))
        .mount(&server)
        .await;

    let models = list_models(&anthropic(&server.uri())).await.unwrap();
    assert_eq!(models, ["claude-test"]);
}
