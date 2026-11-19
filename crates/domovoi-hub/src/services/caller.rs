//! Who asked for a command: an authenticated person and the surface they used.

use domovoi_core::{Actor, UserId};

/// Where an authenticated person's request came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    /// The web UI, with the session's stored one-way hash (never the cookie).
    Web {
        /// The hash that goes into an approval's provenance.
        session_hash: [u8; 32],
    },
    /// A chat platform, for the allowlisted platform user who pressed a button.
    Chat {
        /// The platform, for example `discord`.
        platform: String,
        /// The platform's id of the person.
        platform_user: String,
    },
}

/// Who asked: an authenticated human and the surface they used. Only a transport builds
/// one, after its own authentication; a service never builds or widens one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    user: UserId,
    surface: Surface,
}

impl Caller {
    /// A web caller. Built once, by the session extractor, after the session lookup.
    pub(crate) fn web(user: UserId, session_hash: [u8; 32]) -> Self {
        Self {
            user,
            surface: Surface::Web { session_hash },
        }
    }

    /// A chat caller. Built once, by the interaction authorizer, after the allowlist and
    /// admin lookup.
    pub(crate) fn chat(user: UserId, platform: &str, platform_user: &str) -> Self {
        Self {
            user,
            surface: Surface::Chat {
                platform: platform.to_owned(),
                platform_user: platform_user.to_owned(),
            },
        }
    }

    /// The hub account the person acts as.
    #[must_use]
    pub fn user(&self) -> UserId {
        self.user
    }

    /// The surface the request came from.
    #[must_use]
    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    /// Whether the request came from chat, where a destructive action cannot be approved.
    #[must_use]
    pub fn is_chat(&self) -> bool {
        matches!(self.surface, Surface::Chat { .. })
    }

    /// The actor to record on a change the person made.
    #[must_use]
    pub fn actor(&self) -> Actor {
        Actor::User(self.user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_actor_is_the_user() {
        let user = UserId::new();
        let web = Caller::web(user, [7; 32]);
        let chat = Caller::chat(user, "discord", "42");
        assert_eq!(web.actor(), Actor::User(user));
        assert_eq!(chat.actor(), Actor::User(user));
        assert!(!web.is_chat());
        assert!(chat.is_chat());
    }
}
