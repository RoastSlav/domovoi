//! The identity of an agent's action journal.

use uuid::Uuid;

/// The identity of an agent's action journal, as the agent reported it on `hello`.
///
/// Opaque. The hub compares epochs and never creates one; only the agent's local `journal
/// init` and `journal reset` produce a value.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct JournalEpoch(Uuid);

impl JournalEpoch {
    /// Wraps the epoch an agent reported.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// The epoch as the UUID the wire carries.
    #[must_use]
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for JournalEpoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
