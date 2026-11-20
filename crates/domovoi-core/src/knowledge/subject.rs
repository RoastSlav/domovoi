//! What a fact or relation is about: a typed reference into the inventory or to an entity.

use std::fmt;
use std::str::FromStr;

use crate::id::{EntityId, HostId};

/// A reference to something a fact can be about. It names inventory by host and name rather
/// than copying it, so a fact survives a container being recreated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubjectRef {
    /// A whole host.
    Host(HostId),
    /// A container on a host, by name.
    Container {
        /// The host it runs on.
        host: HostId,
        /// The container name.
        name: String,
    },
    /// A systemd unit on a host, by name.
    Unit {
        /// The host it runs on.
        host: HostId,
        /// The unit name.
        name: String,
    },
    /// A subject the owner invented.
    Entity(EntityId),
}

/// Why subject text was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SubjectError {
    /// The kind before the colon is not one of `host`, `container`, `unit`, `entity`.
    #[error("unknown subject kind")]
    UnknownKind,
    /// An id is not a hyphenated lowercase UUID.
    #[error("the subject id is not a valid id")]
    BadId,
    /// A container or unit name is empty, too long or has forbidden characters.
    #[error("the subject name is not valid")]
    BadName,
}

impl SubjectRef {
    /// The host the subject belongs to; `None` only for an entity.
    #[must_use]
    pub fn host(&self) -> Option<HostId> {
        match self {
            Self::Host(host) | Self::Container { host, .. } | Self::Unit { host, .. } => {
                Some(*host)
            }
            Self::Entity(_) => None,
        }
    }

    /// The entity id when the subject is an entity.
    #[must_use]
    pub fn entity(&self) -> Option<EntityId> {
        match self {
            Self::Entity(id) => Some(*id),
            _ => None,
        }
    }
}

impl fmt::Display for SubjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(host) => write!(f, "host:{host}"),
            Self::Container { host, name } => write!(f, "container:{host}/{name}"),
            Self::Unit { host, name } => write!(f, "unit:{host}/{name}"),
            Self::Entity(id) => write!(f, "entity:{id}"),
        }
    }
}

fn parse_id<T: FromStr>(text: &str) -> Result<T, SubjectError> {
    let lower = text.len() == 36 && !text.chars().any(|c| c.is_ascii_uppercase());
    if !lower {
        return Err(SubjectError::BadId);
    }
    text.parse().map_err(|_| SubjectError::BadId)
}

fn container_name_ok(name: &str) -> bool {
    name.len() <= 128
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

fn unit_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '.' | '@' | '-'))
}

impl FromStr for SubjectRef {
    type Err = SubjectError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (kind, rest) = text.split_once(':').ok_or(SubjectError::UnknownKind)?;
        match kind {
            "host" => Ok(Self::Host(parse_id(rest)?)),
            "entity" => Ok(Self::Entity(parse_id(rest)?)),
            "container" | "unit" => {
                let (host, name) = rest.split_once('/').ok_or(SubjectError::BadName)?;
                let host = parse_id(host)?;
                let is_container = kind == "container";
                let ok = if is_container {
                    container_name_ok(name)
                } else {
                    unit_name_ok(name)
                };
                if !ok {
                    return Err(SubjectError::BadName);
                }
                let name = name.to_owned();
                Ok(if is_container {
                    Self::Container { host, name }
                } else {
                    Self::Unit { host, name }
                })
            }
            _ => Err(SubjectError::UnknownKind),
        }
    }
}

impl serde::Serialize for SubjectRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for SubjectRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_ref_round_trips_and_rejects_bad_text() {
        let host = HostId::new();
        let entity = EntityId::new();
        for subject in [
            SubjectRef::Host(host),
            SubjectRef::Container {
                host,
                name: "plex-1.a".into(),
            },
            SubjectRef::Unit {
                host,
                name: "ssh@x:y.service".into(),
            },
            SubjectRef::Entity(entity),
        ] {
            let text = subject.to_string();
            assert_eq!(text.parse::<SubjectRef>().unwrap(), subject);
            let json = serde_json::to_string(&subject).unwrap();
            assert_eq!(serde_json::from_str::<SubjectRef>(&json).unwrap(), subject);
        }
        let upper = format!("Host:{host}");
        assert_eq!(upper.parse::<SubjectRef>(), Err(SubjectError::UnknownKind));
        assert_eq!("host:nope".parse::<SubjectRef>(), Err(SubjectError::BadId));
        let shouty = format!("host:{}", host.to_string().to_uppercase());
        assert_eq!(shouty.parse::<SubjectRef>(), Err(SubjectError::BadId));
        for bad in ["a/b", "a b", "", "-lead"] {
            assert_eq!(
                format!("container:{host}/{bad}").parse::<SubjectRef>(),
                Err(SubjectError::BadName),
                "{bad}"
            );
        }
    }
}
