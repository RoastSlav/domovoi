//! [`DiscordGateway`]: posting and updating cards, opening threads and replying, through
//! Discord's HTTP API; [`ChatGateway::run`] hands events to [`super::events`].

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::Card;
use domovoi_core::ports::{ChatError, ChatGateway, Clock, InteractionSink, MessageRef, ThreadRef};
use secrecy::{ExposeSecret, SecretString};
use twilight_http::Client;
use twilight_http::api_error::ApiError;
use twilight_http::error::ErrorType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

use super::components::card_buttons;
use super::embed::card_embed;
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
    token: SecretString,
    http: Arc<Client>,
    settings: DiscordSettings,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for DiscordGateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordGateway")
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
    /// A gateway posting to `settings.channel_id` as the bot with `token`. Nothing connects
    /// until a call or [`ChatGateway::run`]. TLS uses the process-wide rustls provider.
    #[must_use]
    pub fn new(token: SecretString, settings: DiscordSettings, clock: Arc<dyn Clock>) -> Self {
        let http = Arc::new(Client::new(token.expose_secret().to_owned()));
        Self {
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

#[cfg(test)]
mod tests {
    use super::*;

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
