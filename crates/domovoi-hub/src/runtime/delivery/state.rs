//! The notification state one delivery pass works on, and how it is stored.

use domovoi_core::ports::{MessageRef, NotifyStateRepo, StoreError};
use domovoi_core::{IncidentId, NotifyState};

use super::Delivery;

/// What a delivery pass works on: the state before the event (`base`, what is stored now) and
/// the state the plan wants after delivery (`current`, which also collects chat references as
/// posts are confirmed).
pub(super) struct Working {
    /// The state as it is stored, before this pass.
    pub(super) base: NotifyState,
    /// The plan's state, gaining chat references as posts are confirmed.
    pub(super) current: NotifyState,
}

/// What a pass delivered.
pub(super) struct Delivered {
    /// Every action on every gateway was confirmed (or there was nothing to deliver).
    pub(super) ok: bool,
    /// `current` after the pass: plan state plus the chat references that were confirmed.
    pub(super) state: NotifyState,
}

/// The state to store after a pass: the plan's state only when every delivery was confirmed,
/// otherwise just the chat references it confirmed, on top of the state before the pass.
pub(super) fn confirmed(base: &NotifyState, delivered: Delivered) -> NotifyState {
    if delivered.ok {
        delivered.state
    } else {
        NotifyState {
            messages: delivered.state.messages,
            ..base.clone()
        }
    }
}

impl Delivery {
    /// The stored notification state of `incident`. `None` means nothing was said about it
    /// yet and becomes the empty default; a read error stays an error.
    pub(super) async fn state(&self, incident: IncidentId) -> Result<NotifyState, StoreError> {
        Ok(NotifyStateRepo::get(&*self.store, incident)
            .await?
            .unwrap_or_default())
    }

    /// The card reference already posted on `platform`, if any.
    pub(super) async fn message(
        &self,
        state: &std::sync::Arc<tokio::sync::Mutex<Working>>,
        platform: &str,
    ) -> Option<MessageRef> {
        state
            .lock()
            .await
            .current
            .messages
            .get(platform)
            .map(|refs| refs.message.clone())
    }

    /// Saves the chat references confirmed so far, on top of the state before the pass, so a
    /// posted card is never posted twice. The write is skipped when nothing changed.
    pub(super) async fn save_refs(&self, incident: IncidentId, working: &Working) {
        let state = NotifyState {
            messages: working.current.messages.clone(),
            ..working.base.clone()
        };
        if state == working.base {
            return;
        }
        self.save(incident, &state, "chat message references").await;
    }

    /// Saves the state a pass confirmed, when it differs from the state before the pass.
    pub(super) async fn save_after(
        &self,
        incident: IncidentId,
        base: &NotifyState,
        delivered: Delivered,
    ) {
        let state = confirmed(base, delivered);
        if state == *base {
            return;
        }
        self.save(incident, &state, "policy state").await;
    }

    /// Saves `state` for `incident` as it stands, for ticks that have nothing to deliver.
    pub(in crate::runtime) async fn save_state(&self, incident: IncidentId, state: &NotifyState) {
        self.save(incident, state, "policy state").await;
    }

    /// Writes `state`, logging and counting a failed write. The next event or tick plans from
    /// the older row, which at worst repeats a delivery.
    async fn save(&self, incident: IncidentId, state: &NotifyState, what: &'static str) {
        if let Err(err) =
            NotifyStateRepo::put(&*self.store, incident, state, self.clock.now()).await
        {
            tracing::error!(%err, %incident, what, "could not save notification state");
            super::state_error("write");
        }
    }
}
