//! Chat interactions: platform events translated into one shape for the hub.
//!
//! These carry no authorization data. The hub's interaction authorizer decides whether the
//! platform user is allowed to do anything.

use jiff::Timestamp;

use crate::card::CardButton;
use crate::id::IncidentId;

/// What the user did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionKind {
    /// Pressed a card button.
    Button {
        /// The button.
        button: CardButton,
        /// The incident whose card it was on.
        incident: IncidentId,
    },
    /// Wrote in an incident thread.
    ThreadMessage {
        /// The incident.
        incident: IncidentId,
        /// What they wrote.
        text: String,
    },
    /// Ran a chat command.
    Command {
        /// Command name.
        name: String,
        /// Arguments as typed.
        args: Vec<String>,
    },
}

/// One event from a chat platform.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Interaction {
    /// Chat platform, for example `discord`.
    pub platform: String,
    /// The platform's user id.
    pub platform_user: String,
    /// What they did.
    pub kind: InteractionKind,
    /// When the gateway received it.
    pub received_at: Timestamp,
}

/// The hub's answer to an interaction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InteractionReply {
    /// Done.
    Accepted {
        /// Optional short confirmation for the user.
        message: Option<String>,
    },
    /// Refused.
    Rejected {
        /// Why, in plain words.
        reason: String,
    },
}
