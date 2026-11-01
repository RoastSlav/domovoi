//! Discord events as [`Interaction`]s: button presses, messages in incident threads and
//! slash commands, each handed to the hub's sink. Nothing here decides who may do what.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{ChatError, Clock, InteractionSink};
use domovoi_core::{IncidentId, Interaction, InteractionKind, InteractionReply};
use serenity::all::{
    Channel, ChannelId, ChannelType, ClientBuilder, ComponentInteraction, Context,
    CreateInteractionResponse, CreateInteractionResponseFollowup, FullEvent, GatewayIntents,
    GuildId, Message, MessageId,
};

use super::commands;
use super::gateway::{PLATFORM, chat_error};
use super::ids::parse_custom_id;
use super::settings::DiscordSettings;

/// The most thread → incident lookups remembered.
pub const THREAD_CACHE: usize = 1024;

/// What a press of anything not ours, or a rejection, tells the user.
pub const NOT_ALLOWED: &str = "Not allowed";

/// What event handlers and commands share.
pub struct Shared {
    /// Where interactions go.
    pub sink: Arc<dyn InteractionSink>,
    /// The channel and web address.
    pub settings: DiscordSettings,
    /// Time, for `received_at`.
    pub clock: Arc<dyn Clock>,
    threads: Mutex<BTreeMap<u64, IncidentId>>,
}

/// The poise user data.
pub type Data = Arc<Shared>;

/// The error type of commands and handlers.
pub type Error = Box<dyn std::error::Error + Send + Sync>;

impl Shared {
    /// An interaction by `user` received now.
    #[must_use]
    pub fn interaction(&self, user: u64, kind: InteractionKind) -> Interaction {
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

async fn on_component(ctx: &Context, shared: &Shared, press: &ComponentInteraction) {
    // Acknowledge within Discord's 3 seconds; the answer follows ephemerally.
    if let Err(err) = press
        .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
        .await
    {
        tracing::warn!(%err, "could not acknowledge a button press");
        return;
    }
    let text = match parse_custom_id(&press.data.custom_id) {
        Some((incident, button)) => {
            let kind = InteractionKind::Button { button, incident };
            let reply = shared
                .sink
                .deliver(shared.interaction(press.user.id.get(), kind))
                .await;
            reply_text(&reply)
        }
        None => NOT_ALLOWED.to_owned(),
    };
    let followup = CreateInteractionResponseFollowup::new()
        .content(text)
        .ephemeral(true);
    if let Err(err) = press.create_followup(&ctx.http, followup).await {
        tracing::warn!(%err, "could not answer a button press");
    }
}

/// The incident whose card started `thread`, read from the card's title link.
async fn incident_of_thread(
    ctx: &Context,
    shared: &Shared,
    thread: ChannelId,
) -> Option<IncidentId> {
    if let Some(incident) = shared.cached(thread.get()) {
        return Some(incident);
    }
    let parent = ChannelId::new(shared.settings.channel_id);
    // A thread started from a message has that message's id.
    let starter = parent
        .message(&ctx.http, MessageId::new(thread.get()))
        .await
        .ok()?;
    let incident = starter.embeds.iter().find_map(|e| {
        e.url
            .as_deref()
            .and_then(|u| shared.settings.incident_of_url(u))
    })?;
    shared.remember(thread.get(), incident);
    Some(incident)
}

async fn on_message(ctx: &Context, shared: &Shared, message: &Message) {
    if message.author.bot || message.guild_id.is_none() || message.content.trim().is_empty() {
        return;
    }
    let Ok(Channel::Guild(channel)) = message.channel_id.to_channel(&ctx.http).await else {
        return;
    };
    let in_our_threads = matches!(
        channel.kind,
        ChannelType::PublicThread | ChannelType::PrivateThread
    ) && channel.parent_id.map(ChannelId::get)
        == Some(shared.settings.channel_id);
    if !in_our_threads {
        return;
    }
    let Some(incident) = incident_of_thread(ctx, shared, message.channel_id).await else {
        return;
    };
    let kind = InteractionKind::ThreadMessage {
        incident,
        text: message.content.clone(),
    };
    // The hub posts its answer in the thread itself; a rejection stays silent.
    let reply = shared
        .sink
        .deliver(shared.interaction(message.author.id.get(), kind))
        .await;
    if let InteractionReply::Rejected { reason } = reply {
        tracing::info!(user = %message.author.id, %reason, "thread message not accepted");
    }
}

/// Registers the slash commands in the guild of the incident channel, so they appear at
/// once rather than after Discord's global propagation.
async fn register(
    ctx: &Context,
    shared: &Shared,
    framework: &poise::Framework<Data, Error>,
) -> Result<(), Error> {
    let channel = ChannelId::new(shared.settings.channel_id)
        .to_channel(&ctx.http)
        .await?;
    let guild: GuildId = channel
        .guild()
        .map(|c| c.guild_id)
        .ok_or("the incident channel is not in a server")?;
    poise::builtins::register_in_guild(ctx, &framework.options().commands, guild).await?;
    Ok(())
}

fn handle<'a>(
    framework: poise::FrameworkContext<'a, Data, Error>,
    event: &'a FullEvent,
) -> poise::BoxFuture<'a, Result<(), Error>> {
    let ctx = framework.serenity_context;
    let shared = framework.user_data;
    Box::pin(async move {
        match event {
            FullEvent::InteractionCreate { interaction } => {
                if let Some(press) = interaction.as_message_component() {
                    on_component(ctx, shared, press).await;
                }
            }
            FullEvent::Message { new_message } => on_message(ctx, shared, new_message).await,
            FullEvent::ShardStageUpdate { event } => {
                tracing::info!(stage = ?event.new, "discord connection");
            }
            _ => {}
        }
        Ok(())
    })
}

/// Connects as the bot with `token` and delivers events to `sink` until the connection
/// ends for good. Reconnects and rate limits are handled by serenity.
pub async fn run(
    token: &str,
    settings: DiscordSettings,
    sink: Arc<dyn InteractionSink>,
    clock: Arc<dyn Clock>,
) -> Result<(), ChatError> {
    let shared = Arc::new(Shared {
        sink,
        settings,
        clock,
        threads: Mutex::new(BTreeMap::new()),
    });
    let setup_shared = shared;
    let options = poise::FrameworkOptions {
        commands: commands::all(),
        event_handler: handle,
        ..Default::default()
    };
    let framework = poise::Framework::builder()
        .options(options)
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                tracing::info!(bot = %ready.user.name, "connected to discord");
                if let Err(err) = register(ctx, &setup_shared, framework).await {
                    tracing::warn!(%err, "could not register slash commands");
                }
                Ok(setup_shared)
            })
        })
        .build();
    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    let mut client = ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .map_err(|e| chat_error(&e))?;
    let result = client.start().await.map_err(|e| chat_error(&e));
    if let Err(err) = &result {
        tracing::warn!(%err, "discord connection ended");
    }
    result
}
