//! The state the registry decides under one lock, and why a registration is refused.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use domovoi_core::{AgentId, HostId};
use tokio::sync::Semaphore;

use super::registry::Entry;

/// Everything the registry decides under one lock.
///
/// The retired set and the live sessions share the lock on purpose: a retirement and the
/// registration it races are decided together, so an agent that authenticated before it was
/// revoked or replaced can never become live afterwards.
#[derive(Default)]
pub(super) struct State {
    hosts: BTreeMap<HostId, Entry>,
    /// Identities that were revoked or replaced. An `AgentId` is never reused, so a
    /// registration of one of these is always a connection that outlived its enrollment.
    pub(super) retired: BTreeSet<AgentId>,
    /// Test support: every registration waits for a permit here before it is decided.
    pub(super) registration_gate: Option<Arc<Semaphore>>,
}

impl Deref for State {
    type Target = BTreeMap<HostId, Entry>;

    fn deref(&self) -> &Self::Target {
        &self.hosts
    }
}

impl DerefMut for State {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.hosts
    }
}

/// Why a session may not be registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegisterError {
    /// The authenticated agent was revoked or replaced after its handshake began.
    Retired(AgentId),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retired(agent) => write!(f, "agent {agent} is retired"),
        }
    }
}
