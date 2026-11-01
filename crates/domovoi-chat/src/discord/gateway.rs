//! [`DiscordGateway`]: posting and updating cards, opening threads and replying, through
//! Discord's HTTP API; [`ChatGateway::run`] hands events to [`super::events`].

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::Card;
use domovoi_core::ports::{ChatError, ChatGateway, Clock, InteractionSink, MessageRef, ThreadRef};
use secrecy::{ExposeSecret, SecretString};
use serenity::all::{
    ChannelId, CreateMessage, CreateThread, EditMessage, Http, HttpError, MessageId,
};

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
const THREAD_EXISTS: isize = 160_004;

/// The Discord bot: cards in one channel, a thread per incident, and interactions handed to
/// the hub's sink.
pub struct DiscordGateway {
    token: SecretString,
    http: Arc<Http>,
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

/// `err` as a [`ChatError`]. Discord's messages never contain the bot token.
pub(crate) fn chat_error(err: &serenity::Error) -> ChatError {
    if let serenity::Error::Http(HttpError::UnsuccessfulRequest(response)) = err {
        let status = response.status_code.as_u16();
        let message = &response.error.message;
        return match status {
            404 => ChatError::NotFound,
            429 => ChatError::RateLimited { retry_after: None },
            400..=499 => ChatError::Rejected(format!("discord {status}: {message}")),
            _ => ChatError::Transport(format!("discord {status}: {message}")),
        };
    }
    ChatError::Transport(err.to_string())
}

fn error_code(err: &serenity::Error) -> Option<isize> {
    match err {
        serenity::Error::Http(HttpError::UnsuccessfulRequest(response)) => {
            Some(response.error.code)
        }
        _ => None,
    }
}

fn id_of(text: &str) -> Result<u64, ChatError> {
    text.parse::<u64>()
        .ok()
        .filter(|id| *id != 0)
        .ok_or_else(|| ChatError::Rejected(format!("{text:?} is not a Discord id")))
}

fn thread_name(title: &str) -> String {
    let title = title.trim();
    let name = if title.is_empty() { "incident" } else { title };
    name.chars().take(MAX_THREAD_NAME).collect()
}

impl DiscordGateway {
    /// A gateway posting to `settings.channel_id` as the bot with `token`. Nothing connects
    /// until a call or [`ChatGateway::run`].
    #[must_use]
    pub fn new(token: SecretString, settings: DiscordSettings, clock: Arc<dyn Clock>) -> Self {
        let http = Arc::new(Http::new(token.expose_secret()));
        Self {
            token,
            http,
            settings,
            clock,
        }
    }

    fn channel(&self) -> ChannelId {
        ChannelId::new(self.settings.channel_id)
    }

    fn message_ref(&self, id: MessageId) -> MessageRef {
        MessageRef {
            platform: PLATFORM.to_owned(),
            id: id.get().to_string(),
        }
    }
}

#[async_trait]
impl ChatGateway for DiscordGateway {
    fn platform(&self) -> &str {
        PLATFORM
    }

    async fn post_card(&self, card: &Card) -> Result<MessageRef, ChatError> {
        let message = CreateMessage::new()
            .embed(card_embed(card, &self.settings))
            .components(card_buttons(card, &self.settings));
        let sent = self
            .channel()
            .send_message(&self.http, message)
            .await
            .map_err(|e| chat_error(&e))?;
        Ok(self.message_ref(sent.id))
    }

    async fn update_card(&self, msg: &MessageRef, card: &Card) -> Result<(), ChatError> {
        let id = MessageId::new(id_of(&msg.id)?);
        let edit = EditMessage::new()
            .embed(card_embed(card, &self.settings))
            .components(card_buttons(card, &self.settings));
        self.channel()
            .edit_message(&self.http, id, edit)
            .await
            .map_err(|e| chat_error(&e))?;
        Ok(())
    }

    async fn open_thread(&self, msg: &MessageRef, title: &str) -> Result<ThreadRef, ChatError> {
        let id = MessageId::new(id_of(&msg.id)?);
        // A thread started from a message has that message's id.
        let thread = ThreadRef {
            platform: PLATFORM.to_owned(),
            id: id.get().to_string(),
        };
        match self
            .channel()
            .create_thread_from_message(&self.http, id, CreateThread::new(thread_name(title)))
            .await
        {
            Ok(_) => Ok(thread),
            Err(e) if error_code(&e) == Some(THREAD_EXISTS) => Ok(thread),
            Err(e) => Err(chat_error(&e)),
        }
    }

    async fn reply(&self, thread: &ThreadRef, text: &str) -> Result<(), ChatError> {
        let channel = ChannelId::new(id_of(&thread.id)?);
        for part in split_message(text, MAX_MESSAGE) {
            channel
                .send_message(&self.http, CreateMessage::new().content(part))
                .await
                .map_err(|e| chat_error(&e))?;
        }
        Ok(())
    }

    async fn run(&self, sink: Arc<dyn InteractionSink>) -> Result<(), ChatError> {
        events::run(
            self.token.expose_secret(),
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
        assert_eq!(id_of("1234"), Ok(1234));
        assert!(id_of("0").is_err());
        assert!(id_of("abc").is_err());
    }
}
