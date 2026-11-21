//! The conversation aggregate root and its limits.

use jiff::Timestamp;

use crate::id::{ConversationId, HostId, OperationId};

/// Longest question, in characters.
pub const MAX_QUESTION_CHARS: usize = 1_000;
/// Most messages kept in one conversation (100 exchanges).
pub const MAX_MESSAGES: usize = 200;
/// Longest conversation title, in characters.
pub const MAX_TITLE_CHARS: usize = 80;
/// Longest stored failure detail, in characters.
pub const MAX_FAILURE_DETAIL_CHARS: usize = 300;

/// What a conversation is about. Always chosen explicitly; there is no default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationScope {
    /// Every host, answered from a summary.
    Global,
    /// One host, whose read tools the model may use while it is connected.
    Host(HostId),
}

impl ConversationScope {
    /// The stored kind name: `"global"` or `"host"`.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Host(_) => "host",
        }
    }

    /// The host of a host scope.
    #[must_use]
    pub const fn host(&self) -> Option<HostId> {
        match self {
            Self::Global => None,
            Self::Host(id) => Some(*id),
        }
    }

    /// Rebuilds a scope from stored parts; `None` for an unknown kind or a host scope
    /// without a host.
    #[must_use]
    pub fn from_parts(kind: &str, host: Option<HostId>) -> Option<Self> {
        match (kind, host) {
            ("global", _) => Some(Self::Global),
            ("host", Some(id)) => Some(Self::Host(id)),
            _ => None,
        }
    }
}

/// One Operator conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    /// Identity.
    pub id: ConversationId,
    /// Empty until the first question, then its first line, redacted, at most
    /// [`MAX_TITLE_CHARS`] characters.
    pub title: String,
    /// What the conversation is about; never changes.
    pub scope: ConversationScope,
    /// The operation attached for read-only context.
    pub operation: Option<OperationId>,
    /// Highest ordinal ever assigned; never decreases, even when retention deletes messages.
    pub last_ordinal: u32,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it last changed.
    pub updated_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_scope_round_trips_its_parts() {
        let host = HostId::new();
        for scope in [ConversationScope::Global, ConversationScope::Host(host)] {
            assert_eq!(
                ConversationScope::from_parts(scope.kind(), scope.host()),
                Some(scope)
            );
        }
        assert_eq!(ConversationScope::from_parts("host", None), None);
        assert_eq!(ConversationScope::from_parts("all", None), None);
    }
}
