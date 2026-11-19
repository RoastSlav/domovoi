//! Operations: durable pieces of work on hosts, made of immutable steps (ARCHITECTURE.md §5).
//!
//! The pure part is [`state::OperationState`], the decision function [`decide::decide`] with
//! its events and results, the audit kinds and the journal payload builders. Nothing there
//! does I/O, reads a clock or draws randomness. The service around it is one writer task
//! that applies every command in one commit ([`OperationService`], [`OperationHandle`]) and
//! an effect runner scoped to operations.

pub mod audit;
pub mod decide;
mod deps;
mod effects;
mod error;
mod handle;
pub mod journal;
mod proposal;
mod settings;
mod startup;
pub mod state;
mod writer;

use std::sync::Arc;

use domovoi_core::ports::AggregateKind;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

pub use decide::{
    AuditNote, DecideError, Decision, EffectSpec, ExecutionProposal, IdSource, JournalNote,
    NewEdge, NewStep, OperationEvent, ProgressChange, derive_status,
};
pub use deps::OperationDeps;
pub use error::OperationError;
pub use handle::{ExecutionRequest, OperationHandle, OperationSeed, StepFinish};
pub use settings::OperationSettings;
pub use state::OperationState;

use crate::effects::{EffectRunner, EffectRunnerDeps, EffectSettings};
use writer::OpShared;

/// The operation service: the writer and the runner of operation effects.
#[derive(Debug)]
pub struct OperationService;

impl OperationService {
    /// Starts one task running the writer, which applies every command one at a time, and
    /// the effect runner for operations. At start, before the runner claims anything, claimed
    /// dispatch effects go back to pending and queued operations are admitted once.
    #[must_use]
    pub fn spawn(
        deps: OperationDeps,
        settings: OperationSettings,
    ) -> (OperationHandle, JoinHandle<()>) {
        let (writer, incoming) = writer::queue();
        let nudge = Arc::new(Notify::new());
        let shared = Arc::new(OpShared {
            deps,
            settings,
            writer,
            nudge: nudge.clone(),
        });
        let mut runner = EffectRunner::new(
            EffectRunnerDeps {
                queue: shared.deps.effects.clone(),
                uow: shared.deps.uow.clone(),
                clock: shared.deps.clock.clone(),
                advisories: shared.deps.advisories.clone(),
                nudge,
                shutdown: shared.deps.shutdown.clone(),
            },
            EffectSettings::default(),
        )
        .scoped(vec![AggregateKind::Operation]);
        effects::register(&mut runner, &shared);
        let service = shared.clone();
        let task = tokio::spawn(async move {
            startup::recover(&service).await;
            let _ = tokio::join!(writer::run(service, incoming), runner.run());
        });
        (OperationHandle { shared }, task)
    }
}
