//! Messages of a conversation and their status vocabulary.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::id::{ConversationId, MessageId, TurnId};

macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($(#[$vmeta])* $variant),+ }

        impl $name {
            /// Stable wire and storage name.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// Reads [`Self::as_str`] back; `None` for anything else.
            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                match text { $($text => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

wire_enum! {
    /// Who wrote a message.
    MessageRole {
        /// The person.
        User => "user",
        /// The model.
        Assistant => "assistant",
    }
}

wire_enum! {
    /// Where a message is in its life. A user message is always `Complete`.
    MessageStatus {
        /// The answer is being written.
        Pending => "pending",
        /// Finished.
        Complete => "complete",
        /// The answer could not be written.
        Failed => "failed",
        /// The person stopped it.
        Cancelled => "cancelled",
    }
}

wire_enum! {
    /// Why an answer failed or was stopped.
    MessageFailure {
        /// The hourly limit or another running answer.
        Busy => "busy",
        /// The monthly budget is spent.
        BudgetSpent => "budget_spent",
        /// Every model provider failed.
        ModelUnavailable => "model_unavailable",
        /// Triage is not running.
        TriageOff => "triage_off",
        /// The hub restarted while the answer was written.
        Interrupted => "interrupted",
        /// The person pressed Stop.
        Cancelled => "cancelled",
        /// A store or task failure.
        Internal => "internal",
    }
}

/// One read tool the model used while answering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTool {
    /// A read tool name from the closed set, or `unknown tool`.
    pub tool: String,
    /// Whether the call failed or was refused.
    pub failed: bool,
}

/// One message of a conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Identity.
    pub id: MessageId,
    /// The conversation it belongs to.
    pub conversation: ConversationId,
    /// 1-based, unique per conversation, increasing.
    pub ordinal: u32,
    /// Who wrote it.
    pub role: MessageRole,
    /// Where it is in its life.
    pub status: MessageStatus,
    /// The question and the answer to it share the turn at creation.
    pub turn: TurnId,
    /// Redacted text; empty while pending, failed or cancelled.
    pub text: String,
    /// Set only when the status is `Failed` or `Cancelled`.
    pub failure: Option<MessageFailure>,
    /// Redacted explanation, cut to `MAX_FAILURE_DETAIL_CHARS`.
    pub failure_detail: Option<String>,
    /// Tools used; assistant messages only.
    pub tools: Vec<MessageTool>,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it settled.
    pub completed_at: Option<Timestamp>,
}

impl Message {
    /// A finished user message.
    #[must_use]
    pub fn user(
        conversation: ConversationId,
        ordinal: u32,
        turn: TurnId,
        text: String,
        at: Timestamp,
    ) -> Self {
        Self {
            id: MessageId::new(),
            conversation,
            ordinal,
            role: MessageRole::User,
            status: MessageStatus::Complete,
            turn,
            text,
            failure: None,
            failure_detail: None,
            tools: Vec::new(),
            created_at: at,
            completed_at: Some(at),
        }
    }

    /// An assistant message waiting for its answer.
    #[must_use]
    pub fn pending_answer(
        conversation: ConversationId,
        ordinal: u32,
        turn: TurnId,
        at: Timestamp,
    ) -> Self {
        Self {
            id: MessageId::new(),
            conversation,
            ordinal,
            role: MessageRole::Assistant,
            status: MessageStatus::Pending,
            turn,
            text: String::new(),
            failure: None,
            failure_detail: None,
            tools: Vec::new(),
            created_at: at,
            completed_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_enums_round_trip_their_wire_strings() {
        for r in [MessageRole::User, MessageRole::Assistant] {
            assert_eq!(MessageRole::parse(r.as_str()), Some(r));
        }
        for s in [
            MessageStatus::Pending,
            MessageStatus::Complete,
            MessageStatus::Failed,
            MessageStatus::Cancelled,
        ] {
            assert_eq!(MessageStatus::parse(s.as_str()), Some(s));
        }
        for f in [
            MessageFailure::Busy,
            MessageFailure::BudgetSpent,
            MessageFailure::ModelUnavailable,
            MessageFailure::TriageOff,
            MessageFailure::Interrupted,
            MessageFailure::Cancelled,
            MessageFailure::Internal,
        ] {
            assert_eq!(MessageFailure::parse(f.as_str()), Some(f));
        }
        assert_eq!(MessageStatus::parse("done"), None);
        assert_eq!(MessageFailure::parse("x"), None);
        assert_eq!(MessageRole::parse("system"), None);
    }

    #[test]
    fn message_defaults_are_pending_free() {
        let c = ConversationId::new();
        let at = Timestamp::UNIX_EPOCH;
        let q = Message::user(c, 1, TurnId::new(), "hi".into(), at);
        assert_eq!(q.status, MessageStatus::Complete);
        assert!(q.failure.is_none() && q.tools.is_empty());
        let a = Message::pending_answer(c, 2, q.turn, at);
        assert_eq!(a.status, MessageStatus::Pending);
        assert!(a.text.is_empty() && a.failure.is_none() && a.completed_at.is_none());
    }
}
