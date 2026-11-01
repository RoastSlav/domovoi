//! Slash commands. Each becomes [`InteractionKind::Command`] for the hub, which answers;
//! the answer is shown only to the user who ran it.

use domovoi_core::InteractionKind;
use poise::CreateReply;

use super::events::{Data, Error, reply_text};

/// A poise command context.
pub type Ctx<'a> = poise::Context<'a, Data, Error>;

/// How long `/mute` mutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum MuteFor {
    /// One hour.
    #[name = "1h"]
    OneHour,
    /// Four hours.
    #[name = "4h"]
    FourHours,
    /// A day.
    #[name = "24h"]
    Day,
}

impl MuteFor {
    /// The duration as the hub reads it.
    #[must_use]
    pub const fn arg(self) -> &'static str {
        match self {
            Self::OneHour => "1h",
            Self::FourHours => "4h",
            Self::Day => "24h",
        }
    }
}

/// Sends `/name args…` to the hub and shows its answer privately.
async fn forward(ctx: Ctx<'_>, name: &str, args: Vec<String>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let shared = ctx.data();
    let kind = InteractionKind::Command {
        name: name.to_owned(),
        args,
    };
    let reply = shared
        .sink
        .deliver(shared.interaction(ctx.author().id.get(), kind))
        .await;
    ctx.send(
        CreateReply::default()
            .content(reply_text(&reply))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// Hosts and their open incidents.
#[poise::command(slash_command, guild_only)]
pub async fn status(ctx: Ctx<'_>) -> Result<(), Error> {
    forward(ctx, "status", Vec::new()).await
}

/// Open incidents, newest first.
#[poise::command(slash_command, guild_only)]
pub async fn incidents(ctx: Ctx<'_>) -> Result<(), Error> {
    forward(ctx, "incidents", Vec::new()).await
}

/// Mutes an incident.
#[poise::command(slash_command, guild_only)]
pub async fn mute(
    ctx: Ctx<'_>,
    #[description = "The incident's short id, from its card"] incident: String,
    #[description = "For how long"] duration: MuteFor,
) -> Result<(), Error> {
    forward(ctx, "mute", vec![incident, duration.arg().to_owned()]).await
}

/// Sends today's digest now.
#[poise::command(slash_command, guild_only)]
pub async fn digest(ctx: Ctx<'_>) -> Result<(), Error> {
    forward(ctx, "digest", Vec::new()).await
}

/// Every command the bot registers.
#[must_use]
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![status(), incidents(), mute(), digest()]
}
