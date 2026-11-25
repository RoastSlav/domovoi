//! The nonces the hub issued to helpers and has not yet seen answered.
//!
//! Nonces live in memory only: a hub restart merely forces the next request to ask again.

use std::sync::{Mutex, PoisonError};

use domovoi_core::HostId;
use domovoi_core::attest::{NONCE_TTL, Nonce32};
use jiff::Timestamp;

/// Why a nonce could not be consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonceError {
    /// No such nonce was issued, or it was already consumed.
    Unknown,
    /// The nonce was issued to another host.
    OtherHost,
    /// The nonce is [`NONCE_TTL`] old or older.
    Expired,
}

impl NonceError {
    /// The audit and log detail token.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Unknown => "nonce_unknown",
            Self::OtherHost => "nonce_other_host",
            Self::Expired => "nonce_expired",
        }
    }
}

struct Issued {
    host: HostId,
    nonce: Nonce32,
    at: Timestamp,
}

/// Issued, unanswered nonces.
#[derive(Default)]
pub struct NonceStore {
    issued: Mutex<Vec<Issued>>,
}

impl NonceStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Issued>> {
        self.issued.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Issues a fresh random nonce for `host` at `now`, and drops every nonce that has
    /// expired.
    pub fn issue(&self, host: HostId, now: Timestamp) -> Nonce32 {
        self.issue_bytes(host, now, rand::random())
    }

    /// Issues the nonce made of `bytes`. Split from [`NonceStore::issue`] so a test can pick
    /// the bytes.
    pub fn issue_bytes(&self, host: HostId, now: Timestamp, bytes: [u8; 32]) -> Nonce32 {
        let nonce = Nonce32::from_bytes(bytes);
        let mut issued = self.lock();
        issued.retain(|entry| now.duration_since(entry.at) < NONCE_TTL);
        issued.push(Issued {
            host,
            nonce,
            at: now,
        });
        nonce
    }

    /// Consumes `nonce` for `host` and returns the time it was issued. An unknown nonce, a
    /// nonce of another host and an expired one are refused; the boundary is strict, so an
    /// age of exactly [`NONCE_TTL`] is expired. A nonce that exists is consumed whichever
    /// way the check goes, so it is never usable twice.
    ///
    /// # Errors
    ///
    /// [`NonceError`] naming why it was refused.
    pub fn take(
        &self,
        host: HostId,
        nonce: &Nonce32,
        now: Timestamp,
    ) -> Result<Timestamp, NonceError> {
        let mut issued = self.lock();
        let Some(index) = issued.iter().position(|entry| entry.nonce == *nonce) else {
            return Err(NonceError::Unknown);
        };
        if issued[index].host != host {
            return Err(NonceError::OtherHost);
        }
        let entry = issued.remove(index);
        if now.duration_since(entry.at) >= NONCE_TTL {
            return Err(NonceError::Expired);
        }
        Ok(entry.at)
    }
}
