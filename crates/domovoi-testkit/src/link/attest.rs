//! The helper attestation half of `FakeAgentLink`: a scripted answer per request, the nonces
//! the hub asked with, and the helper facts a live session announced.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::Nonce32;
use domovoi_core::ports::{
    AgentLink, AttestationReply, HelperRelay, HelperReport, LinkError, Presence,
};
use jiff::Timestamp;

use super::FakeAgentLink;
use crate::lock;

/// What a test scripts for one `attest_safety` call: it sees the nonce the hub sent.
pub type AttestHandler =
    Arc<dyn Fn(HostId, Nonce32) -> Result<AttestationReply, LinkError> + Send + Sync>;

/// The scripted attestation state of one fake link.
#[derive(Default)]
pub(super) struct AttestState {
    handlers: Mutex<BTreeMap<HostId, AttestHandler>>,
    reports: Mutex<BTreeMap<HostId, HelperReport>>,
    asked: Mutex<Vec<(HostId, Nonce32)>>,
}

impl AttestState {
    /// Records the request and runs the host's handler, or refuses when none is scripted.
    pub(super) fn answer(
        &self,
        host: HostId,
        nonce: Nonce32,
    ) -> Result<AttestationReply, LinkError> {
        lock(&self.asked).push((host, nonce));
        let handler = lock(&self.handlers).get(&host).cloned();
        match handler {
            Some(handler) => handler(host, nonce),
            None => Err(LinkError::Rejected(
                "no attestation handler is scripted".to_owned(),
            )),
        }
    }

    pub(super) fn report(&self, host: HostId) -> Option<HelperReport> {
        lock(&self.reports).get(&host).cloned()
    }
}

impl FakeAgentLink {
    /// Scripts how `host` answers `attest_safety`.
    pub fn script_attestation(&self, host: HostId, handler: AttestHandler) {
        lock(&self.attest.handlers).insert(host, handler);
    }

    /// Sets what `host`'s live session says about its helper.
    pub fn set_helper_report(&self, host: HostId, report: HelperReport) {
        lock(&self.attest.reports).insert(host, report);
    }

    /// Every `(host, nonce)` the hub asked an attestation with, oldest first.
    #[must_use]
    pub fn attestation_requests(&self) -> Vec<(HostId, Nonce32)> {
        lock(&self.attest.asked).clone()
    }
}

#[async_trait]
impl HelperRelay for FakeAgentLink {
    async fn ask(
        &self,
        host: HostId,
        nonce: Nonce32,
        deadline: Timestamp,
    ) -> Result<AttestationReply, LinkError> {
        AgentLink::attest_safety(self, host, nonce, deadline).await
    }

    fn report(&self, host: HostId) -> Option<HelperReport> {
        AgentLink::helper_report(self, host)
    }

    fn connected(&self, host: HostId) -> bool {
        matches!(AgentLink::presence(self, host), Presence::Connected { .. })
    }
}
