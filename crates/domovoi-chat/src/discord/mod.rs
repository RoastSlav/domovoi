//! Discord: incident cards as embeds with buttons, a thread per incident, follow-up
//! messages, slash commands, and one-way messages such as digests, through a webhook or
//! the bot (ADR 0010).
//!
//! The gateway only translates. Every press, thread message and command goes to the
//! [`InteractionSink`](domovoi_core::ports::InteractionSink), which decides whether the
//! Discord user may do anything.

pub mod commands;
pub mod components;
pub mod embed;
pub mod events;
pub mod gateway;
pub mod ids;
pub mod settings;
pub mod split;
pub mod webhook;

pub use gateway::{DiscordGateway, PLATFORM};
pub use settings::DiscordSettings;
pub use webhook::DiscordWebhookNotifier;
