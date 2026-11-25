//! The helper trust use cases: what the hub shows about a host's helper, asking again, and the
//! owner's pin and unpin of the helper key.
//!
//! Pinning is the owner's alone: it needs the web surface, the account password (again) and
//! the full fingerprint typed out, compared with the one the hub computed from the key it
//! holds. Nothing here pins on first contact, and no chat, model or policy path reaches it.

use std::sync::Arc;

use domovoi_core::attest::TrustView;
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{Clock, HostRepo, StoreError};
use domovoi_core::{HostId, HostName};
use subtle::ConstantTimeEq;

use super::{Caller, StepUp, Surface};
use crate::auth::{AuthError, AuthState};
use crate::helper_trust::{HelperDetail, HelperTrustHub, RECHECK_MIN_GAP};

/// What the hub shows about one host's helper.
#[derive(Debug, Clone)]
pub struct HelperView {
    /// The host's name.
    pub host: HostName,
    /// The host's id.
    pub host_id: HostId,
    /// Everything the hub computed.
    pub detail: HelperDetail,
}

/// Why a helper trust command or read was refused. Nothing changed unless the variant says
/// so.
#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    /// No such host.
    #[error("no such host")]
    NoSuchHost,
    /// Pinning is done in the web UI by a signed-in person only.
    #[error("the helper key is pinned from the web UI only")]
    WebOnly,
    /// The hub holds no key from this host's helper.
    #[error("the hub holds no helper key from this host")]
    NoHelperKey,
    /// The typed text is not the full fingerprint of the held key.
    #[error("type the full fingerprint of the helper key")]
    FingerprintMismatch,
    /// The request carried no password.
    #[error("pinning a helper key needs your password")]
    PasswordRequired,
    /// The password is wrong.
    #[error("wrong password")]
    BadPassword,
    /// Too many wrong passwords from this address.
    #[error("too many attempts; try again in {retry_after_secs} s")]
    TooMany {
        /// Seconds until the next attempt.
        retry_after_secs: i64,
    },
    /// The host's agent is not connected, so nothing can be asked.
    #[error("the host is not connected")]
    Offline,
    /// A request for this host went out a moment ago.
    #[error("the helper was asked a moment ago; try again in {retry_after_secs} s")]
    RecheckTooSoon {
        /// Seconds until a re-check is accepted.
        retry_after_secs: i64,
    },
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// What the helper trust use cases are built from.
pub struct HelperTrustDeps {
    /// The trust machinery.
    pub hub: Arc<HelperTrustHub>,
    /// Hosts, read only.
    pub hosts: Arc<dyn HostRepo>,
    /// Asks for the password again.
    pub auth: Arc<AuthState>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// The helper trust use cases.
#[derive(Clone)]
pub struct HelperTrust {
    hub: Arc<HelperTrustHub>,
    hosts: Arc<dyn HostRepo>,
    auth: Arc<AuthState>,
    clock: Arc<dyn Clock>,
}

/// A typed fingerprint with spaces and case ignored; `None` unless it is 64 hex characters.
fn normalised(typed: &str) -> Option<String> {
    let text: String = typed
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    (text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())).then_some(text)
}

impl HelperTrust {
    /// The use cases over `deps`.
    #[must_use]
    pub fn new(deps: HelperTrustDeps) -> Self {
        Self {
            hub: deps.hub,
            hosts: deps.hosts,
            auth: deps.auth,
            clock: deps.clock,
        }
    }

    async fn host(&self, name: &HostName) -> Result<HostId, HelperError> {
        Ok(self
            .hosts
            .get_host_by_name(name)
            .await?
            .ok_or(HelperError::NoSuchHost)?
            .id)
    }

    fn web_session(caller: &Caller) -> Result<(), HelperError> {
        match caller.surface() {
            Surface::Web { .. } => Ok(()),
            Surface::Chat { .. } => Err(HelperError::WebOnly),
        }
    }

    /// What the hub shows about `host`'s helper.
    ///
    /// # Errors
    /// No such host, or a store failure.
    pub async fn view(&self, host: HostName) -> Result<HelperView, HelperError> {
        let host_id = self.host(&host).await?;
        let detail = self.hub.detail(host_id).await?;
        Ok(HelperView {
            host,
            host_id,
            detail,
        })
    }

    /// Asks the host's helper for an attestation now. Refused for a host that is not
    /// connected and, under [`RECHECK_MIN_GAP`] after the last request, for a host that was
    /// just asked.
    ///
    /// # Errors
    /// No such host, offline, asked too soon, or a store failure.
    pub async fn recheck(&self, caller: &Caller, host: HostName) -> Result<(), HelperError> {
        Self::web_session(caller)?;
        let id = self.host(&host).await?;
        if !self.hub.relay.connected(id) {
            return Err(HelperError::Offline);
        }
        if let Some(last) = self.hub.last_request_at(id) {
            let age = self.clock.now().duration_since(last);
            if age < RECHECK_MIN_GAP {
                let wait = RECHECK_MIN_GAP.as_secs() - age.as_secs();
                return Err(HelperError::RecheckTooSoon {
                    retry_after_secs: wait.max(1),
                });
            }
        }
        self.hub.request(id).await?;
        Ok(())
    }

    /// Asks `host` now and returns the trust view the answer left, for a containment
    /// request that needs a fresh attestation.
    ///
    /// # Errors
    /// A store failure.
    pub async fn fresh(&self, _caller: &Caller, host: HostId) -> Result<TrustView, HelperError> {
        // A host that is already being asked answers busy; the view below reads what that
        // request leaves.
        self.hub.request(host).await?;
        Ok(self.hub.stored_with_live(host).await?.view)
    }

    async fn step_up(&self, caller: &Caller, proof: StepUp<'_>) -> Result<(), HelperError> {
        let Some(password) = proof.password else {
            return Err(HelperError::PasswordRequired);
        };
        match self
            .auth
            .confirm_password(proof.ip, caller.user(), password)
            .await
        {
            Ok(()) => Ok(()),
            Err(AuthError::LockedOut { retry_after_secs }) => {
                Err(HelperError::TooMany { retry_after_secs })
            }
            Err(_) => Err(HelperError::BadPassword),
        }
    }

    /// Pins the key the hub holds from `host`'s helper, when the typed fingerprint equals the
    /// one the hub computed from that key.
    ///
    /// # Errors
    /// In order: not the web surface, no such host, no key held, a missing or wrong
    /// password, a typed text that is not the full fingerprint of the held key.
    pub async fn pin(
        &self,
        caller: &Caller,
        host: HostName,
        typed_fingerprint: &str,
        proof: StepUp<'_>,
    ) -> Result<HelperView, HelperError> {
        Self::web_session(caller)?;
        let id = self.host(&host).await?;
        let held = self
            .hub
            .relay
            .report(id)
            .and_then(|report| report.public_key)
            .ok_or(HelperError::NoHelperKey)?;
        self.step_up(caller, proof).await?;
        let computed = KeyFingerprint::of_public_key(&held).to_string();
        let typed = normalised(typed_fingerprint).ok_or(HelperError::FingerprintMismatch)?;
        if !bool::from(typed.as_bytes().ct_eq(computed.as_bytes())) {
            return Err(HelperError::FingerprintMismatch);
        }
        self.hub.pin_key(caller.actor(), id, held).await?;
        self.view(host).await
    }

    /// Removes the pin and the verified state of `host`. Containment is blocked again until
    /// the key is pinned.
    ///
    /// # Errors
    /// Not the web surface, no such host, a missing or wrong password, or a store failure.
    pub async fn unpin(
        &self,
        caller: &Caller,
        host: HostName,
        proof: StepUp<'_>,
    ) -> Result<HelperView, HelperError> {
        Self::web_session(caller)?;
        let id = self.host(&host).await?;
        self.step_up(caller, proof).await?;
        self.hub.unpin_key(caller.actor(), id).await?;
        self.view(host).await
    }
}
