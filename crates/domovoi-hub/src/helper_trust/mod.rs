//! Helper trust: asking a host's helper for a signed safety report, verifying it against the
//! key the owner pinned, and keeping the latest outcome the gate reads.
//!
//! This is support code behind [`crate::services::HelperTrust`]; it holds no handle an HTTP
//! or chat handler could reach. The hub never trusts what an agent says about its helper: the
//! helper (root) signs a report of the effective safety set, the agent relays the bytes
//! unchanged, and the hub verifies them against the pinned key only.
//!
//! Verification follows a fixed order ([`verify`]): decode, pin, signature over the received
//! bytes, parse, hashes, nonce (consumed late), signed-incomplete, generation, high-water
//! mark. Trust follows the latest outcome; no per-host flag stays set once an honest report
//! arrives.

mod accept;
mod detail;
mod gate_view;
mod nonce;
mod pin;
mod request;
mod schedule;
#[cfg(test)]
mod tests;
mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use domovoi_core::HostId;
use domovoi_core::ports::{Clock, HelperRelay, HelperTrustRepo, UnitOfWork};
use jiff::Timestamp;

pub use detail::{
    AttestationDetail, AttestationState, HelperDetail, KeyState, ReceivedKey, VerifiedDetail,
};
pub use nonce::{NonceError, NonceStore};
pub use request::{Attempt, RECHECK_MIN_GAP};
pub use schedule::{ATTEST_PERIOD, AttestOnEvent, spawn_schedule};

/// What the hub last tried for one host, kept in memory for display and rate limiting.
#[derive(Debug, Clone, Default)]
pub(crate) struct LastAttempt {
    /// When the hub last sent a request.
    pub at: Option<Timestamp>,
    /// The unsigned token the agent relayed in answer, an unverified hint.
    pub hint: Option<String>,
}

/// The helper trust machinery over the store, the unit of work and the live relay.
pub struct HelperTrustHub {
    pub(crate) repo: Arc<dyn HelperTrustRepo>,
    pub(crate) uow: Arc<dyn UnitOfWork>,
    pub(crate) relay: Arc<dyn HelperRelay>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) nonces: NonceStore,
    in_flight: Mutex<BTreeSet<HostId>>,
    attempts: Mutex<BTreeMap<HostId, LastAttempt>>,
}

impl HelperTrustHub {
    /// The trust machinery over `repo` and `uow`, asking through `relay`.
    #[must_use]
    pub fn new(
        repo: Arc<dyn HelperTrustRepo>,
        uow: Arc<dyn UnitOfWork>,
        relay: Arc<dyn HelperRelay>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            repo,
            uow,
            relay,
            clock,
            nonces: NonceStore::new(),
            in_flight: Mutex::new(BTreeSet::new()),
            attempts: Mutex::new(BTreeMap::new()),
        }
    }

    /// The nonce store, for tests that issue a nonce by hand.
    #[must_use]
    pub fn nonces(&self) -> &NonceStore {
        &self.nonces
    }

    pub(crate) fn attempt_of(&self, host: HostId) -> LastAttempt {
        self.attempts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&host)
            .cloned()
            .unwrap_or_default()
    }

    /// When the hub last sent a request for `host`, if it has.
    #[must_use]
    pub fn last_request_at(&self, host: HostId) -> Option<Timestamp> {
        self.attempt_of(host).at
    }

    pub(crate) fn note_attempt(&self, host: HostId, at: Option<Timestamp>, hint: Option<String>) {
        let mut attempts = self.attempts.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = attempts.entry(host).or_default();
        if let Some(at) = at {
            entry.at = Some(at);
        }
        entry.hint = hint;
    }

    /// Takes the per-host in-flight slot; `None` when a request for `host` is already running.
    pub(crate) fn begin(&self, host: HostId) -> Option<InFlight<'_>> {
        let mut set = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        set.insert(host).then(|| InFlight { hub: self, host })
    }
}

/// The in-flight slot of one host, released when dropped.
pub(crate) struct InFlight<'a> {
    hub: &'a HelperTrustHub,
    host: HostId,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.hub
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.host);
    }
}
