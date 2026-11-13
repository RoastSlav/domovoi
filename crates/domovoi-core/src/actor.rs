//! Who did something: a typed identity stored in audit and journal rows in its string form.

use std::fmt;
use std::str::FromStr;

use crate::id::{HostId, UserId};

/// The incidents component, the default journal actor for timeline mirrors.
pub const SYSTEM_INCIDENTS: &str = "incidents";
/// The action writer.
pub const SYSTEM_ACTIONS: &str = "actions";
/// The audit verifier, which records a broken chain at hub start.
pub const SYSTEM_AUDIT: &str = "audit";

/// Who did something. Stored in audit and journal rows in its string form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Actor {
    /// A human user.
    User(UserId),
    /// A Domovoi component, named in kebab or snake case (for example `actions`, `incidents`).
    System(String),
    /// An agent, by the host it runs on.
    Agent(HostId),
    /// A policy rule, by its id (rules arrive in a later release).
    Policy(String),
}

impl Actor {
    /// A system actor; the component name is validated like `FromStr` does.
    pub fn system(component: &str) -> Result<Self, ActorParseError> {
        if valid_name(component) {
            Ok(Self::System(component.to_owned()))
        } else {
            Err(ActorParseError::InvalidName(component.to_owned()))
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(id) => write!(f, "user:{id}"),
            Self::System(name) => write!(f, "system:{name}"),
            Self::Agent(id) => write!(f, "agent:{id}"),
            Self::Policy(rule) => write!(f, "policy:{rule}"),
        }
    }
}

impl FromStr for Actor {
    type Err = ActorParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let Some((kind, value)) = text.split_once(':') else {
            return Err(ActorParseError::UnknownKind(text.to_owned()));
        };
        match kind {
            "user" => Ok(Self::User(value.parse()?)),
            "agent" => Ok(Self::Agent(value.parse()?)),
            "system" if valid_name(value) => Ok(Self::System(value.to_owned())),
            "policy" if valid_name(value) => Ok(Self::Policy(value.to_owned())),
            "system" | "policy" => Err(ActorParseError::InvalidName(value.to_owned())),
            _ => Err(ActorParseError::UnknownKind(kind.to_owned())),
        }
    }
}

impl serde::Serialize for Actor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Actor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Why an actor string could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActorParseError {
    /// The part before the first colon is not a known actor kind.
    #[error("unknown actor kind {0:?}")]
    UnknownKind(String),
    /// A system or policy name is empty, too long or has a character outside `[a-z0-9._-]`.
    #[error("invalid actor name {0:?}")]
    InvalidName(String),
    /// The id after `user:` or `agent:` is not a UUID.
    #[error("invalid actor id: {0}")]
    Id(uuid::Error),
}

impl From<uuid::Error> for ActorParseError {
    fn from(err: uuid::Error) -> Self {
        Self::Id(err)
    }
}

/// A system or policy name: 1 to 64 characters from `[a-z0-9._-]`.
fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> Actor {
        Actor::User(UserId::from_uuid(uuid::Uuid::from_u128(
            0x0190_a0a0_0000_7000_8000_0000_0000_0001,
        )))
    }

    fn agent() -> Actor {
        Actor::Agent(HostId::from_uuid(uuid::Uuid::from_u128(
            0x0190_a0a0_0000_7000_8000_0000_0000_0002,
        )))
    }

    #[test]
    fn all_four_variants_round_trip() {
        for actor in [
            user(),
            Actor::System("actions".to_owned()),
            agent(),
            Actor::Policy("rule-1".to_owned()),
        ] {
            let text = actor.to_string();
            assert_eq!(text.parse::<Actor>().unwrap(), actor, "{text}");
        }
    }

    #[test]
    fn string_forms_are_stable() {
        assert_eq!(
            user().to_string(),
            "user:0190a0a0-0000-7000-8000-000000000001"
        );
        assert_eq!(
            Actor::System("actions".to_owned()).to_string(),
            "system:actions"
        );
        assert_eq!(
            agent().to_string(),
            "agent:0190a0a0-0000-7000-8000-000000000002"
        );
        assert_eq!(
            Actor::Policy("rule-1".to_owned()).to_string(),
            "policy:rule-1"
        );
    }

    #[test]
    fn rejects_a_legacy_chat_actor() {
        assert_eq!(
            "chat:discord:1".parse::<Actor>(),
            Err(ActorParseError::UnknownKind("chat".to_owned()))
        );
    }

    #[test]
    fn rejects_empty_overlong_and_uppercase_names() {
        let empty = String::new();
        let long = "a".repeat(65);
        for text in [
            format!("system:{empty}"),
            format!("system:{long}"),
            "system:Actions".to_owned(),
            "system:has space".to_owned(),
            "policy:Rule".to_owned(),
        ] {
            assert!(
                matches!(text.parse::<Actor>(), Err(ActorParseError::InvalidName(_))),
                "{text}"
            );
        }
        assert!(Actor::system("actions").is_ok());
        assert!(Actor::system(&long).is_err());
    }

    #[test]
    fn rejects_a_user_that_is_not_a_uuid() {
        assert!(matches!(
            "user:not-a-uuid".parse::<Actor>(),
            Err(ActorParseError::Id(_))
        ));
        assert!(matches!(
            "agent:not-a-uuid".parse::<Actor>(),
            Err(ActorParseError::Id(_))
        ));
        assert!(matches!(
            "nocolon".parse::<Actor>(),
            Err(ActorParseError::UnknownKind(_))
        ));
    }

    #[test]
    fn serde_uses_the_display_form() {
        for actor in [
            user(),
            Actor::System("incidents".to_owned()),
            agent(),
            Actor::Policy("rule-1".to_owned()),
        ] {
            let json = serde_json::to_string(&actor).unwrap();
            assert_eq!(json, format!("\"{actor}\""));
            assert_eq!(serde_json::from_str::<Actor>(&json).unwrap(), actor);
        }
    }
}
