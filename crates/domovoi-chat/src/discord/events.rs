//! Discord events as [`Interaction`]s: button presses, messages in incident threads and
//! slash commands, each handed to the hub's sink. Nothing here decides who may do what.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{ChatError, Clock, InteractionSink};
use domovoi_core::{IncidentId, Interaction, InteractionKind, InteractionReply};
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt as _};
use twilight_http::Client;
use twilight_model::application::interaction::{
    Interaction as DiscordInteraction, InteractionData,
};
use twilight_model::channel::message::MessageFlags;
use twilight_model::channel::{ChannelType, Message};
use twilight_model::gateway::payload::incoming::Ready;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::Id;
use twilight_model::id::marker::ChannelMarker;

use super::commands::{command_kind, definitions};
use super::gateway::PLATFORM;
use super::ids::parse_custom_id;
use super::settings::DiscordSettings;

/// The most thread → incident lookups remembered.
pub const THREAD_CACHE: usize = 1024;

/// What a press of anything not ours, or a rejection without a reason, tells the user.
pub const NOT_ALLOWED: &str = "Not allowed";

/// What the event loop shares.
pub struct Shared {
    sink: Arc<dyn InteractionSink>,
    http: Arc<Client>,
    settings: DiscordSettings,
    clock: Arc<dyn Clock>,
    threads: Mutex<BTreeMap<u64, IncidentId>>,
}

/// What to show the user for `reply`: the hub's message, "Done." when it has none, or the
/// hub's reason for a refusal ("Not allowed" when it gives none).
#[must_use]
pub fn reply_text(reply: &InteractionReply) -> String {
    match reply {
        InteractionReply::Accepted { message } => {
            message.clone().unwrap_or_else(|| "Done.".to_owned())
        }
        InteractionReply::Rejected { reason } if reason.trim().is_empty() => NOT_ALLOWED.to_owned(),
        InteractionReply::Rejected { reason } => reason.clone(),
    }
}

impl Shared {
    fn interaction(&self, user: u64, kind: InteractionKind) -> Interaction {
        Interaction {
            platform: PLATFORM.to_owned(),
            platform_user: user.to_string(),
            kind,
            received_at: self.clock.now(),
        }
    }

    fn remember(&self, thread: u64, incident: IncidentId) {
        let mut threads = self
            .threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if threads.len() >= THREAD_CACHE {
            threads.pop_first();
        }
        threads.insert(thread, incident);
    }

    fn cached(&self, thread: u64) -> Option<IncidentId> {
        self.threads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&thread)
            .copied()
    }

    fn channel(&self) -> Option<Id<ChannelMarker>> {
        Id::new_checked(self.settings.channel_id)
    }

    /// Registers the slash commands in the incident channel's server, so they appear at
    /// once rather than after Discord's global propagation.
    async fn register(&self, ready: &Ready) -> Result<(), String> {
        let channel = self.channel().ok_or("the channel id is 0")?;
        let guild = self
            .http
            .channel(channel)
            .await
            .map_err(|e| e.to_string())?
            .model()
            .await
            .map_err(|e| e.to_string())?
            .guild_id
            .ok_or("the incident channel is not in a server")?;
        self.http
            .interaction(ready.application.id)
            .set_guild_commands(guild, &definitions())
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Acknowledges `i` within Discord's 3 seconds, asks the sink, and answers privately.
    async fn answer(&self, i: &DiscordInteraction, kind: Option<InteractionKind>, update: bool) {
        let response = if update {
            InteractionResponse {
                kind: InteractionResponseType::DeferredUpdateMessage,
                data: None,
            }
        } else {
            InteractionResponse {
                kind: InteractionResponseType::DeferredChannelMessageWithSource,
                data: Some(InteractionResponseData {
                    flags: Some(MessageFlags::EPHEMERAL),
                    ..InteractionResponseData::default()
                }),
            }
        };
        let client = self.http.interaction(i.application_id);
        if let Err(err) = client.create_response(i.id, &i.token, &response).await {
            tracing::warn!(%err, "could not acknowledge an interaction");
            return;
        }
        let text = match (kind, i.author_id()) {
            (Some(kind), Some(user)) => {
                let reply = self.sink.deliver(self.interaction(user.get(), kind)).await;
                reply_text(&reply)
            }
            _ => NOT_ALLOWED.to_owned(),
        };
        if let Err(err) = client
            .create_followup(&i.token)
            .content(&text)
            .flags(MessageFlags::EPHEMERAL)
            .await
        {
            tracing::warn!(%err, "could not answer an interaction");
        }
    }

    async fn on_interaction(&self, i: &DiscordInteraction) {
        match &i.data {
            Some(InteractionData::MessageComponent(data)) => {
                let kind = parse_custom_id(&data.custom_id)
                    .map(|(incident, button)| InteractionKind::Button { button, incident });
                self.answer(i, kind, true).await;
            }
            Some(InteractionData::ApplicationCommand(data)) => {
                self.answer(i, command_kind(data), false).await;
            }
            _ => {}
        }
    }

    /// The incident whose card started `thread`, read from the card's title link.
    async fn incident_of_thread(&self, thread: Id<ChannelMarker>) -> Option<IncidentId> {
        if let Some(incident) = self.cached(thread.get()) {
            return Some(incident);
        }
        let channel = self.http.channel(thread).await.ok()?.model().await.ok()?;
        let in_our_channel = matches!(
            channel.kind,
            ChannelType::PublicThread | ChannelType::PrivateThread
        ) && channel.parent_id == self.channel();
        if !in_our_channel {
            return None;
        }
        // A thread started from a message has that message's id.
        let starter = self
            .http
            .message(self.channel()?, thread.cast())
            .await
            .ok()?
            .model()
            .await
            .ok()?;
        let incident = starter.embeds.iter().find_map(|e| {
            e.url
                .as_deref()
                .and_then(|u| self.settings.incident_of_url(u))
        })?;
        self.remember(thread.get(), incident);
        Some(incident)
    }

    async fn on_message(&self, message: &Message) {
        let ours = self.channel() == Some(message.channel_id);
        if message.author.bot
            || message.guild_id.is_none()
            || ours
            || message.content.trim().is_empty()
        {
            return;
        }
        let Some(incident) = self.incident_of_thread(message.channel_id).await else {
            return;
        };
        let kind = InteractionKind::ThreadMessage {
            incident,
            text: message.content.clone(),
        };
        // The hub posts its answer in the thread itself; a rejection stays silent.
        let reply = self
            .sink
            .deliver(self.interaction(message.author.id.get(), kind))
            .await;
        if let InteractionReply::Rejected { reason } = reply {
            tracing::info!(user = %message.author.id, %reason, "thread message not accepted");
        }
    }
}

/// Connects as the bot with `token` and delivers events to `sink` until the connection
/// ends for good. The shard reconnects on its own and Twilight's HTTP client honours rate
/// limits.
pub async fn run(
    token: &str,
    http: Arc<Client>,
    settings: DiscordSettings,
    sink: Arc<dyn InteractionSink>,
    clock: Arc<dyn Clock>,
) -> Result<(), ChatError> {
    let shared = Arc::new(Shared {
        sink,
        http,
        settings,
        clock,
        threads: Mutex::new(BTreeMap::new()),
    });
    let intents = Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::MESSAGE_CONTENT;
    let mut shard = Shard::new(ShardId::ONE, token.to_owned(), intents);
    let wanted = EventTypeFlags::READY
        | EventTypeFlags::INTERACTION_CREATE
        | EventTypeFlags::MESSAGE_CREATE
        | EventTypeFlags::GATEWAY_RECONNECT;
    while let Some(item) = shard.next_event(wanted).await {
        let event = match item {
            Ok(event) => event,
            Err(err) => {
                tracing::warn!(%err, "discord connection trouble");
                continue;
            }
        };
        let shared = shared.clone();
        match event {
            Event::Ready(ready) => {
                tracing::info!(bot = %ready.user.name, "connected to discord");
                if let Err(err) = shared.register(&ready).await {
                    tracing::warn!(%err, "could not register slash commands");
                }
            }
            Event::GatewayReconnect => tracing::warn!("discord asked to reconnect"),
            Event::InteractionCreate(i) => {
                tokio::spawn(async move { shared.on_interaction(&i.0).await });
            }
            Event::MessageCreate(m) => {
                tokio::spawn(async move { shared.on_message(&m).await });
            }
            _ => {}
        }
    }
    Err(ChatError::Transport(
        "the discord connection closed".to_owned(),
    ))
}
