//! The handle everyone else uses to reach the incident manager.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{IngestError, SignalSink};
use domovoi_core::{IncidentId, Signal, UserId};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};

use super::command::{Command, CommandError};

/// Commands that may wait for the manager before senders wait too.
pub const COMMAND_QUEUE: usize = 1024;

/// A command and where its result goes.
pub(crate) struct Envelope {
    pub(crate) command: Command,
    pub(crate) reply: Option<oneshot::Sender<Result<(), CommandError>>>,
}

/// Sends commands to the incident manager. Cheap to clone. The manager stops once every
/// handle is dropped.
#[derive(Debug, Clone)]
pub struct ManagerHandle {
    pub(crate) tx: mpsc::Sender<Envelope>,
    pub(crate) flaps: Arc<AtomicU64>,
}

impl std::fmt::Debug for Envelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Envelope")
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

impl ManagerHandle {
    /// Sends `command` and waits for it to be stored and published.
    pub async fn send(&self, command: Command) -> Result<(), CommandError> {
        let (reply, result) = oneshot::channel();
        let envelope = Envelope {
            command,
            reply: Some(reply),
        };
        self.tx
            .send(envelope)
            .await
            .map_err(|_| CommandError::Unavailable)?;
        result.await.map_err(|_| CommandError::Unavailable)?
    }

    /// Correlates `signals`, in order.
    pub async fn ingest(&self, signals: Vec<Signal>) -> Result<(), CommandError> {
        self.send(Command::Ingest(signals)).await
    }

    /// Records that `by` acknowledged `incident`.
    pub async fn ack(&self, incident: IncidentId, by: UserId) -> Result<(), CommandError> {
        self.send(Command::Ack { incident, by }).await
    }

    /// Mutes `incident` until `until`, on behalf of `by`.
    pub async fn mute(
        &self,
        incident: IncidentId,
        until: Timestamp,
        by: UserId,
    ) -> Result<(), CommandError> {
        self.send(Command::Mute {
            incident,
            until,
            by,
        })
        .await
    }

    /// Lifts the mute on `incident`.
    pub async fn unmute(&self, incident: IncidentId) -> Result<(), CommandError> {
        self.send(Command::Unmute { incident }).await
    }

    /// Expires mutes and resolves quiet incidents now; the ticker does this on its own.
    pub async fn tick(&self) -> Result<(), CommandError> {
        self.send(Command::Tick).await
    }

    /// Pending incidents dropped before confirmation since the manager started.
    #[must_use]
    pub fn flaps(&self) -> u64 {
        self.flaps.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl SignalSink for ManagerHandle {
    /// Queues the batch and waits until it is stored. A full queue is `Unavailable`, so
    /// agents spool rather than block.
    async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError> {
        let (reply, result) = oneshot::channel();
        let envelope = Envelope {
            command: Command::Ingest(signals),
            reply: Some(reply),
        };
        self.tx.try_send(envelope).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => {
                IngestError::Unavailable("the incident manager is busy".to_owned())
            }
            mpsc::error::TrySendError::Closed(_) => {
                IngestError::Unavailable("the incident manager is not running".to_owned())
            }
        })?;
        match result.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err @ (CommandError::Store(_) | CommandError::Unavailable))) => {
                Err(IngestError::Unavailable(err.to_string()))
            }
            Ok(Err(err)) => Err(IngestError::Rejected(err.to_string())),
            Err(_) => Err(IngestError::Unavailable(
                "the incident manager stopped".to_owned(),
            )),
        }
    }
}
