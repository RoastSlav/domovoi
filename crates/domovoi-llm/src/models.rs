//! Model discovery through the provider's HTTP API.

use std::collections::BTreeSet;

use domovoi_core::ports::LlmError;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::anthropic::{API_VERSION, DEFAULT_BASE_URL};
use crate::http;
use crate::settings::{ProviderKind, ProviderSettings};

#[derive(Deserialize)]
struct WireModels {
    data: Vec<WireModel>,
}

#[derive(Deserialize)]
struct WireModel {
    id: String,
}

/// Lists model IDs available to the configured API key, sorted and deduplicated.
/// CLI providers have no model-list API.
pub async fn list_models(settings: &ProviderSettings) -> Result<Vec<String>, LlmError> {
    let client =
        http::client(settings.timeout).map_err(|e| LlmError::Unavailable(e.to_string()))?;
    let key = settings.api_key.as_ref();
    let request = match settings.kind {
        ProviderKind::Anthropic => {
            let base = settings
                .base_url
                .as_ref()
                .map_or_else(|| DEFAULT_BASE_URL.to_owned(), ToString::to_string);
            let url = format!("{}/v1/models", base.trim_end_matches('/'));
            client
                .get(url)
                .header("x-api-key", required_key(key)?.expose_secret())
                .header("anthropic-version", API_VERSION)
        }
        ProviderKind::OpenaiCompatible => {
            let base = settings
                .base_url
                .as_ref()
                .ok_or_else(|| LlmError::Unavailable("a base URL is required".to_owned()))?;
            let mut request = client.get(http::endpoint(base, "models"));
            if let Some(key) = key {
                request = request.bearer_auth(key.expose_secret());
            }
            request
        }
        ProviderKind::Cli => {
            return Err(LlmError::Unavailable(
                "CLI providers do not have a model-list API".to_owned(),
            ));
        }
    };
    let response: WireModels = http::send(request, key).await?;
    Ok(response
        .data
        .into_iter()
        .map(|model| model.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn required_key(key: Option<&SecretString>) -> Result<&SecretString, LlmError> {
    key.ok_or_else(|| LlmError::Unavailable("an API key is required".to_owned()))
}
