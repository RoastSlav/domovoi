//! Test support for Domovoi.
//!
//! In-memory implementations of every `domovoi-core` port, a fake clock,
//! scripted LLM responses, a recording chat gateway and fixture loaders. Only
//! ever used as a dev-dependency (ARCHITECTURE.md R5).
//!
//! The [`contracts`] module holds one suite per repository port. Each suite exercises the
//! behaviour documented on the port, so the same suite proves both the in-memory fake here
//! and the SQLite adapter correct.

pub mod approvals;
pub mod build;
pub mod chat;
pub mod clock;
pub mod contracts;
pub mod fs;
pub mod link;
pub mod llm;
pub mod process;
pub mod redact;
pub mod repos;

pub use build::BuildError;
pub use chat::{GatewayCall, RecordingGateway, RecordingNotifier};
pub use clock::FakeClock;
pub use contracts::ContractError;
pub use fs::FixtureHostFs;
pub use link::{FakeAgentLink, RecordingSignalSink};
pub use llm::ScriptedLlm;
pub use process::FakeProcessExecutor;
pub use redact::ListRedactor;
pub use repos::InMemoryStore;

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, recovering the data if a panicking test poisoned it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
