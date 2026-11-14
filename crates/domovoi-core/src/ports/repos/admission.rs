//! The persisted rolling window of model-run starts, so the hourly model cap survives a
//! hub restart.

use async_trait::async_trait;
use jiff::{SignedDuration, Timestamp};

use super::StoreError;

/// Who is asking for a model slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionPurpose {
    /// A triage run.
    Triage,
    /// A follow-up question or a chat turn.
    Question,
}

impl AdmissionPurpose {
    /// The stored name, `triage` or `question`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Triage => "triage",
            Self::Question => "question",
        }
    }
}

/// What [`AdmissionRepo::admit`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionOutcome {
    /// A start was recorded at `now`.
    Admitted,
    /// The window already holds `room` starts; `starts` are the recorded starts inside the
    /// window, oldest first, so the caller can say when a slot frees.
    Full {
        /// Every start inside the window, oldest first.
        starts: Vec<Timestamp>,
    },
}

/// The persisted hourly run window shared by triage, follow-up questions and chat.
#[async_trait]
pub trait AdmissionRepo: Send + Sync {
    /// Atomically: delete starts older than twice `window`; read the starts newer than
    /// `now - window`; when fewer than `room` are there, insert one for `purpose` at `now`.
    /// `room == 0` always returns [`AdmissionOutcome::Full`]. Check and insert are one
    /// transaction.
    async fn admit(
        &self,
        purpose: AdmissionPurpose,
        room: u32,
        window: SignedDuration,
        now: Timestamp,
    ) -> Result<AdmissionOutcome, StoreError>;

    /// Starts newer than `since`, oldest first. Read only.
    async fn starts_since(&self, since: Timestamp) -> Result<Vec<Timestamp>, StoreError>;
}
