//! [`DiscordWebhookNotifier`]: one-way messages such as the digest, posted to a channel
//! webhook. Used for digests, and as the fallback when the bot is down.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use domovoi_core::Severity;
use domovoi_core::ports::{ChatError, Clock, Notifier, OutboundMessage};
use jiff::SignedDuration;
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use super::embed::MAX_TITLE;
use super::split::{MAX_MESSAGE, split_message};

/// How long one webhook request may take.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// How many times one message is tried when Discord keeps rate-limiting.
pub const MAX_TRIES: u32 = 3;

/// The longest total wait for rate limits on one message.
pub const MAX_WAIT: SignedDuration = SignedDuration::from_secs(60);

/// A Discord channel webhook. The URL contains its own secret, so it is never logged or
/// put in errors.
pub struct DiscordWebhookNotifier {
    id: String,
    url: SecretString,
    client: reqwest::Client,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for DiscordWebhookNotifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordWebhookNotifier")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// The colour of a message's title embed: by severity, or the accent for information.
fn colour(severity: Option<Severity>) -> u32 {
    match severity {
        Some(Severity::Critical) => 0x00C9_2A2A,
        Some(Severity::Warning) => 0x00E6_7700,
        Some(Severity::Info) | None => 0x003B_5BDB,
    }
}

/// The JSON bodies that carry `msg`: the first has the title as an embed, and the
/// Markdown is split into messages Discord accepts.
#[must_use]
pub fn payloads(msg: &OutboundMessage) -> Vec<Value> {
    let title: String = msg.title.chars().take(MAX_TITLE).collect();
    let embed = json!({ "title": title, "color": colour(msg.severity) });
    let parts = split_message(&msg.markdown, MAX_MESSAGE);
    if parts.is_empty() {
        return vec![json!({ "embeds": [embed] })];
    }
    parts
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                json!({ "content": part, "embeds": [embed] })
            } else {
                json!({ "content": part })
            }
        })
        .collect()
}

/// How long Discord asked to wait, from the JSON body or the `Retry-After` header.
fn retry_after(headers: &reqwest::header::HeaderMap, body: &[u8]) -> Option<SignedDuration> {
    let from_body = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("retry_after").and_then(Value::as_f64));
    let from_header = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok());
    let seconds = from_body.or(from_header)?;
    (seconds.is_finite() && seconds >= 0.0).then(|| SignedDuration::from_secs_f64(seconds))
}

impl DiscordWebhookNotifier {
    /// A notifier called `id` posting to the webhook `url`. TLS uses the process-wide
    /// rustls provider, which the binary installs.
    pub fn new(
        id: impl Into<String>,
        url: SecretString,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, ChatError> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("domovoi/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ChatError::Transport(format!("http client: {}", e.without_url())))?;
        Ok(Self {
            id: id.into(),
            url,
            client,
            clock,
        })
    }

    /// Posts one body, waiting out rate limits within [`MAX_TRIES`] and [`MAX_WAIT`].
    async fn post(&self, body: &Value) -> Result<(), ChatError> {
        let mut waited = SignedDuration::ZERO;
        for attempt in 1..=MAX_TRIES {
            let response = self
                .client
                .post(self.url.expose_secret())
                .json(body)
                .send()
                .await
                .map_err(|e| ChatError::Transport(e.without_url().to_string()))?;
            let status = response.status();
            if status.is_success() {
                return Ok(());
            }
            let headers = response.headers().clone();
            let bytes = response.bytes().await.unwrap_or_default();
            if status != StatusCode::TOO_MANY_REQUESTS {
                let reason = format!("discord webhook answered {}", status.as_u16());
                return Err(if status.is_client_error() {
                    ChatError::Rejected(reason)
                } else {
                    ChatError::Transport(reason)
                });
            }
            let wait = retry_after(&headers, &bytes).unwrap_or(SignedDuration::from_secs(1));
            let out_of_budget = waited
                .checked_add(wait)
                .is_none_or(|total| total > MAX_WAIT);
            if attempt == MAX_TRIES || out_of_budget {
                return Err(ChatError::RateLimited {
                    retry_after: Some(wait),
                });
            }
            tracing::info!(notifier = %self.id, wait = %wait, "discord webhook rate limited");
            let now = self.clock.now();
            self.clock
                .sleep_until(now.checked_add(wait).unwrap_or(now))
                .await;
            waited = waited.checked_add(wait).unwrap_or(MAX_WAIT);
        }
        Err(ChatError::RateLimited { retry_after: None })
    }
}

#[async_trait]
impl Notifier for DiscordWebhookNotifier {
    fn id(&self) -> &str {
        &self.id
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<(), ChatError> {
        for body in payloads(msg) {
            self.post(&body).await?;
        }
        Ok(())
    }
}
