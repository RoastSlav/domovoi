//! Typed identifiers: UUIDv7 newtypes so ids of different entities cannot be mixed up.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a fresh, time-ordered (UUIDv7) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID, for example one read from storage.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::from_str(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a host.
    HostId
);
define_id!(
    /// Identifies an enrolled agent.
    AgentId
);
define_id!(
    /// Identifies an incident.
    IncidentId
);
define_id!(
    /// Identifies a single signal observation.
    SignalId
);
define_id!(
    /// Identifies a proposed or executed action.
    ActionId
);
define_id!(
    /// Identifies a human user.
    UserId
);
define_id!(
    /// Identifies a note attached to an incident.
    NoteId
);
define_id!(
    /// Identifies a hub-to-agent request.
    RequestId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_display_and_from_str() {
        let id = HostId::new();
        let parsed: HostId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn new_ids_are_time_ordered_v7() {
        let id = IncidentId::new();
        assert_eq!(id.as_uuid().get_version_num(), 7);
    }
}
