//! What the writer and the handlers share, and the queue into the writer.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_core::HostId;
use tokio::sync::{Notify, mpsc};

use super::OperationCommand;
use crate::operations::deps::OperationDeps;
use crate::operations::settings::OperationSettings;

/// Commands waiting to be applied queue up at most this deep.
const WRITE_QUEUE: usize = 64;

/// What the writer and the handlers share: the ports, the settings and the way to reach the
/// writer.
pub(crate) struct OpShared {
    pub(crate) deps: OperationDeps,
    pub(crate) settings: OperationSettings,
    /// The queue into the writer task.
    pub(crate) writer: mpsc::Sender<OperationCommand>,
    /// Wakes the effect runner as soon as a commit creates an effect that is due.
    pub(crate) nudge: Arc<Notify>,
    /// What investigations need, when the service runs them.
    pub(crate) investigate: Option<Arc<crate::operations::investigate::Runtime>>,
    /// One gate per host, held from before a dispatch is committed until the request has
    /// been handed to the agent's outgoing queue, and by reconciliation while it reads the
    /// execution. That keeps a committed-but-unsent run from being read as one the agent
    /// never got.
    pub(crate) dispatch_gates: Mutex<BTreeMap<HostId, Arc<tokio::sync::Mutex<()>>>>,
}

impl OpShared {
    /// The host's dispatch gate; see [`OpShared::dispatch_gates`].
    pub(crate) async fn dispatch_gate(&self, host: HostId) -> tokio::sync::OwnedMutexGuard<()> {
        let gate = {
            let mut gates = self
                .dispatch_gates
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gates
                .entry(host)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        gate.lock_owned().await
    }
}

/// Opens the queue into the writer; the receiver goes to [`run`].
pub(crate) fn queue() -> (
    mpsc::Sender<OperationCommand>,
    mpsc::Receiver<OperationCommand>,
) {
    mpsc::channel(WRITE_QUEUE)
}
