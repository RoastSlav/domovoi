//! [`DiscordGateway`]: posting and updating cards, opening threads and replying, through
//! Discord's HTTP API; [`ChatGateway::run`] hands events to [`super::events`]. It is also a
//! [`Notifier`], so one-way messages such as the digest can go out without a webhook.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::Card;
use domovoi_core::ports::{
    ChatError, ChatGateway, Clock, InteractionSink, MessageRef, Notifier, OutboundMessage,
    ThreadRef,
};
use secrecy::{ExposeSecret, SecretString};
use twilight_http::Client;
use twilight_http::api_error::ApiError;
use twilight_http::error::ErrorType;
use twilight_model::channel::message::AllowedMentions;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

use super::components::card_buttons;
use super::embed::{card_embed, message_embed};
use super::events;
use super::settings::DiscordSettings;
use super::split::{MAX_MESSAGE, split_message};

/// The platform name in refs and interactions.
pub const PLATFORM: &str = "discord";

/// The longest thread name Discord accepts, in characters.
pub const MAX_THREAD_NAME: usize = 100;

/// Discord's error code for "a thread has already been created for this message".
const THREAD_EXISTS: u64 = 160_004;

/// The Discord bot: cards in one channel, a thread per incident, and interactions handed to
/// the hub's sink.
pub struct DiscordGateway {
    id: String,
    token: SecretString,
    http: Arc<Client>,
    settings: DiscordSettings,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for DiscordGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordGateway")
            .field("id", &self.id)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// `err` as a [`ChatError`]. Discord's error messages never contain the bot token.
pub(crate) fn chat_error(err: &twilight_http::Error) -> ChatError {
    match err.kind() {
        ErrorType::Response { status, error, .. } => {
            let status = status.get();
            match status {
                404 => ChatError::NotFound,
                429 => ChatError::RateLimited { retry_after: None },
                400..=499 => ChatError::Rejected(format!("discord {status}: {error}")),
                _ => ChatError::Transport(format!("discord {status}: {error}")),
            }
        }
        ErrorType::Unauthorized => ChatError::Rejected("discord rejected the bot token".to_owned()),
        _ => ChatError::Transport(format!("discord request failed: {err}")),
    }
}

/// Discord's JSON error code, when the response carried one.
fn error_code(err: &twilight_http::Error) -> Option<u64> {
    match err.kind() {
        ErrorType::Response {
            error: ApiError::General(general),
            ..
        } => Some(general.code),
        _ => None,
    }
}

fn id_of<T>(text: &str) -> Result<Id<T>, ChatError> {
    text.parse::<u64>()
        .ok()
        .and_then(Id::new_checked)
        .ok_or_else(|| ChatError::Rejected(format!("{text:?} is not a Discord id")))
}

fn thread_name(title: &str) -> String {
    let title = title.trim();
    let name = if title.is_empty() { "incident" } else { title };
    name.chars().take(MAX_THREAD_NAME).collect()
}

impl DiscordGateway {
    /// A gateway called `id` posting to `settings.channel_id` as the bot with `token`.
    /// Nothing connects until a call or [`ChatGateway::run`]. TLS uses the process-wide
    /// rustls provider.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        token: SecretString,
        settings: DiscordSettings,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let http = Arc::new(Client::new(token.expose_secret().to_owned()));
        Self {
            id: id.into(),
            token,
            http,
            settings,
            clock,
        }
    }

    fn channel(&self) -> Result<Id<ChannelMarker>, ChatError> {
        Id::new_checked(self.settings.channel_id)
            .ok_or_else(|| ChatError::Rejected("the channel id is 0".to_owned()))
    }
}

#[async_trait]
impl ChatGateway for DiscordGateway {
    fn platform(&self) -> &str {
        PLATFORM
    }

    async fn post_card(&self, card: &Card) -> Result<MessageRef, ChatError> {
        let embeds = [card_embed(card, &self.settings)];
        let components = card_buttons(card, &self.settings);
        let sent = self
            .http
            .create_message(self.channel()?)
            .embeds(&embeds)
            .components(&components)
            .await
            .map_err(|e| chat_error(&e))?
            .model()
            .await
            .map_err(|e| ChatError::Transport(format!("discord response: {e}")))?;
        Ok(MessageRef {
            platform: PLATFORM.to_owned(),
            id: sent.id.get().to_string(),
        })
    }

    async fn update_card(&self, msg: &MessageRef, card: &Card) -> Result<(), ChatError> {
        let id: Id<MessageMarker> = id_of(&msg.id)?;
        let embeds = [card_embed(card, &self.settings)];
        let components = card_buttons(card, &self.settings);
        self.http
            .update_message(self.channel()?, id)
            .embeds(Some(&embeds))
            .components(Some(&components))
            .await
            .map_err(|e| chat_error(&e))?;
        Ok(())
    }

    async fn open_thread(&self, msg: &MessageRef, title: &str) -> Result<ThreadRef, ChatError> {
        let id: Id<MessageMarker> = id_of(&msg.id)?;
        // A thread started from a message has that message's id.
        let thread = ThreadRef {
            platform: PLATFORM.to_owned(),
            id: id.get().to_string(),
        };
        let name = thread_name(title);
        match self
            .http
            .create_thread_from_message(self.channel()?, id, &name)
            .await
        {
            Ok(_) => Ok(thread),
            Err(e) if error_code(&e) == Some(THREAD_EXISTS) => Ok(thread),
            Err(e) => Err(chat_error(&e)),
        }
    }

    async fn reply(&self, thread: &ThreadRef, text: &str) -> Result<(), ChatError> {
        let channel: Id<ChannelMarker> = id_of(&thread.id)?;
        for part in split_message(text, MAX_MESSAGE) {
            self.http
                .create_message(channel)
                .content(&part)
                .await
                .map_err(|e| chat_error(&e))?;
        }
        Ok(())
    }

    async fn run(&self, sink: Arc<dyn InteractionSink>) -> Result<(), ChatError> {
        events::run(
            self.token.expose_secret(),
            self.http.clone(),
            self.settings.clone(),
            sink,
            self.clock.clone(),
        )
        .await
    }
}

#[async_trait]
impl Notifier for DiscordGateway {
    fn id(&self) -> &str {
        &self.id
    }

    fn replaces_held_cards(&self) -> bool {
        false
    }

    /// Posts the title as an embed on the first message and the Markdown as message text,
    /// split to fit. Text pings where an embed does not, so mentions are turned off.
    async fn send(&self, msg: &OutboundMessage) -> Result<(), ChatError> {
        let channel = self.channel()?;
        let embeds = [message_embed(msg)];
        let no_pings = AllowedMentions::default();
        let parts = split_message(&msg.markdown, MAX_MESSAGE);
        let mut parts = parts.iter();
        let opening = self
            .http
            .create_message(channel)
            .allowed_mentions(Some(&no_pings))
            .embeds(&embeds);
        let opening = match parts.next() {
            Some(first) => opening.content(first),
            None => opening,
        };
        opening.await.map_err(|e| chat_error(&e))?;
        for part in parts {
            self.http
                .create_message(channel)
                .allowed_mentions(Some(&no_pings))
                .content(part)
                .await
                .map_err(|e| chat_error(&e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::Severity;
    use domovoi_testkit::FakeClock;
    use serde_json::{Value, json};
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    use super::*;

    const CHANNEL: u64 = 1234;
    const TITLE: &str = "Daily digest for 2026-11-01";

    fn settings() -> DiscordSettings {
        DiscordSettings {
            channel_id: CHANNEL,
            web_base: Url::parse("https://domovoi.example.net").unwrap(),
        }
    }

    fn clock() -> Arc<FakeClock> {
        Arc::new(FakeClock::new("2026-11-01T08:00:00Z".parse().unwrap()))
    }

    fn bot(server: &MockServer) -> DiscordGateway {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = Client::builder()
            .token("Bot test-token".to_owned())
            .proxy(server.address().to_string(), true)
            .ratelimiter(None)
            .build();
        DiscordGateway {
            id: "discord-gateway".to_owned(),
            token: SecretString::from("test-token".to_owned()),
            http: Arc::new(http),
            settings: settings(),
            clock: clock(),
        }
    }

    async fn discord(status: u16) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/api/v10/channels/{CHANNEL}/messages")))
            .respond_with(ResponseTemplate::new(status).set_body_string("{}"))
            .mount(&server)
            .await;
        server
    }

    fn bodies(requests: &[Request]) -> Vec<Value> {
        requests
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    fn message(markdown: String) -> OutboundMessage {
        OutboundMessage {
            title: TITLE.to_owned(),
            markdown,
            severity: Some(Severity::Warning),
        }
    }

    fn long_digest() -> String {
        let line = format!("- {}", "x".repeat(97));
        vec![line; 60].join("\n")
    }

    #[tokio::test]
    async fn long_digest_goes_out_as_several_messages_with_one_title() {
        let server = discord(200).await;
        let digest = long_digest();
        assert!(digest.chars().count() > 4096, "too long for one embed");
        bot(&server).send(&message(digest.clone())).await.unwrap();

        let sent = bodies(&server.received_requests().await.unwrap());
        assert!(sent.len() >= 3, "{} messages", sent.len());
        assert_eq!(sent[0]["embeds"][0]["title"], TITLE);
        assert_eq!(sent[0]["embeds"][0]["color"], 0x00E6_7700);
        assert!(sent[0]["embeds"][0].get("description").is_none());
        assert!(
            sent[1..].iter().all(|body| body.get("embeds").is_none()),
            "only the first message has the title"
        );
        let parts: Vec<&str> = sent
            .iter()
            .map(|body| body["content"].as_str().unwrap())
            .collect();
        assert!(parts.iter().all(|p| p.chars().count() <= MAX_MESSAGE));
        assert_eq!(parts.join("\n"), digest, "split on lines, nothing lost");
    }

    #[tokio::test]
    async fn empty_message_is_sent_as_the_title_alone() {
        let server = discord(200).await;
        bot(&server).send(&message(String::new())).await.unwrap();
        let sent = bodies(&server.received_requests().await.unwrap());
        assert_eq!(sent.len(), 1);
        assert!(sent[0].get("content").is_none());
        assert_eq!(sent[0]["embeds"][0]["title"], TITLE);
    }

    #[tokio::test]
    async fn every_part_of_a_message_turns_mentions_off() {
        let server = discord(200).await;
        bot(&server).send(&message(long_digest())).await.unwrap();
        let sent = bodies(&server.received_requests().await.unwrap());
        assert!(sent.len() >= 2);
        for body in sent {
            assert_eq!(body["allowed_mentions"]["parse"], json!([]));
        }
    }

    #[tokio::test]
    async fn refused_message_is_rejected_without_the_token() {
        let server = discord(403).await;
        let err = bot(&server)
            .send(&message("all quiet".to_owned()))
            .await
            .unwrap_err();
        assert!(matches!(err, ChatError::Rejected(_)), "{err:?}");
        assert!(!format!("{err} {err:?}").contains("test-token"));
    }

    #[tokio::test]
    async fn notifier_keeps_its_configured_id_and_never_replaces_held_cards() {
        let token = SecretString::from("test-token".to_owned());
        let gateway = DiscordGateway::new("bot-notifier", token, settings(), clock());
        assert_eq!(Notifier::id(&gateway), "bot-notifier");
        assert!(!gateway.replaces_held_cards());
    }

    #[test]
    fn thread_names_are_trimmed_and_capped() {
        assert_eq!(thread_name("  Disk full "), "Disk full");
        assert_eq!(thread_name(""), "incident");
        assert_eq!(
            thread_name(&"é".repeat(150)).chars().count(),
            MAX_THREAD_NAME
        );
    }

    #[test]
    fn ids_must_be_discord_snowflakes() {
        assert_eq!(id_of::<MessageMarker>("1234").unwrap().get(), 1234);
        assert!(id_of::<MessageMarker>("0").is_err());
        assert!(id_of::<MessageMarker>("abc").is_err());
    }
}
