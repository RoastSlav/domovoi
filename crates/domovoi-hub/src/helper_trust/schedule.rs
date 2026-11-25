//! When the hub asks: every fifteen minutes for each connected host whose helper reports
//! present, and when a host authenticates or reports new runtime status.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::ports::{HelperLive, HostReconciler, HostRepo};
use jiff::SignedDuration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{Attempt, HelperTrustHub};
use crate::runtime::every::every;

/// How often a connected host with a helper is asked for a fresh attestation.
pub const ATTEST_PERIOD: SignedDuration = SignedDuration::from_secs(15 * 60);

impl HelperTrustHub {
    /// Asks every connected host whose helper reports present. A failure of one host is
    /// logged and does not stop the others.
    pub async fn request_all(&self, hosts: &dyn HostRepo) {
        let listed = match hosts.list_hosts().await {
            Ok(listed) => listed,
            Err(err) => {
                tracing::warn!(%err, "could not list hosts for the helper attestation pass");
                return;
            }
        };
        for host in listed {
            if self.relay.connected(host.id) && self.helper_present(host.id) {
                self.request_logged(host.id).await;
            }
        }
    }

    fn helper_present(&self, host: HostId) -> bool {
        self.relay
            .report(host)
            .is_some_and(|report| matches!(report.live, HelperLive::Ok { .. }))
    }

    /// Asks `host` and logs the result; a host without a pinned key is not asked.
    pub async fn request_logged(&self, host: HostId) {
        match self.request(host).await {
            Ok(Attempt::Verified | Attempt::NotPinned | Attempt::Busy) => {}
            Ok(other) => tracing::info!(%host, outcome = ?other, "helper attestation not verified"),
            Err(err) => tracing::warn!(%host, %err, "helper attestation failed"),
        }
    }
}

/// Starts the fifteen-minute pass: one pass at once, then one each period, until `shutdown`.
pub fn spawn_schedule(
    hub: Arc<HelperTrustHub>,
    hosts: Arc<dyn HostRepo>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    let clock = hub.clock.clone();
    tokio::spawn(every(clock, ATTEST_PERIOD, shutdown, move || {
        let hub = hub.clone();
        let hosts = hosts.clone();
        async move { hub.request_all(hosts.as_ref()).await }
    }))
}

/// Asks a host for an attestation when it authenticates or reports new runtime status.
pub struct AttestOnEvent {
    hub: Arc<HelperTrustHub>,
}

impl AttestOnEvent {
    /// A reconciler that refreshes `hub`'s attestation of the host.
    #[must_use]
    pub fn new(hub: Arc<HelperTrustHub>) -> Self {
        Self { hub }
    }
}

#[async_trait]
impl HostReconciler for AttestOnEvent {
    async fn reconcile_host(&self, host: HostId) {
        if self.hub.helper_present(host) {
            self.hub.request_logged(host).await;
        }
    }
}
