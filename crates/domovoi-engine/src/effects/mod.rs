//! The effect runner: claims due work on the clock and hands each effect to the handler for
//! its kind.
//!
//! Effects are a work outbox, not an event store: an owner writes one in the same transaction
//! as the state change that requires it, and the handler reports the result back through that
//! owner's writer, which commits the state change and the effect's completion together. A
//! handler therefore never changes effect state here; it either commits through its owner
//! (`Handled`) or asks for another attempt (`Retry`) or gives up (`Dead`).

mod backoff;
mod runner;
#[cfg(test)]
mod tests;

pub use backoff::{BASE, CAP, backoff};
pub use runner::{EffectRunner, EffectRunnerDeps};

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectRecord};
use jiff::SignedDuration;

/// What a handler tells the runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectOutcome {
    /// The handler committed the effect's completion itself through the owner's writer.
    Handled,
    /// A transport-level failure: try again after the backoff.
    Retry {
        /// Why the attempt failed.
        error: String,
    },
    /// A permanent failure: dead-letter it.
    Dead {
        /// Why the effect cannot succeed.
        error: String,
    },
}

/// Runs one kind of effect.
#[async_trait]
pub trait EffectHandler: Send + Sync {
    /// Runs one claimed effect. A handler that finishes the work must complete the effect in
    /// the same unit-of-work commit as the state change, using `claim`; it returns
    /// [`EffectOutcome::Handled`] after that commit succeeded or was refused as stale.
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome;
}

/// How the runner claims work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSettings {
    /// How long a claim is held before another runner may reclaim it. It must outlast the
    /// longest handler: a model call runs the whole tool loop, where one provider call may
    /// take its own five-minute deadline.
    pub lease: SignedDuration,
    /// How often the runner looks for due work when nothing nudges it.
    pub poll: SignedDuration,
    /// How many effects one claim pass takes.
    pub batch: u16,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            lease: SignedDuration::from_mins(15),
            poll: SignedDuration::from_secs(5),
            batch: 16,
        }
    }
}
