//! Wiring the helper trust machinery into the runtime: the trust hub over the store and the
//! link, its use cases, the events that make it ask, and its fifteen-minute pass.

use std::sync::Arc;

use domovoi_core::ports::{Clock, HelperRelay, HostReconciler};
use domovoi_store::Store;
use tokio_util::sync::CancellationToken;

use crate::agent_server::AgentServer;
use crate::auth::AuthState;
use crate::helper_trust::{AttestOnEvent, HelperTrustHub, spawn_schedule};
use crate::services::{HelperTrust, HelperTrustDeps};

/// The trust hub: what the pinned key and the verified attestation prove, which coverage
/// reads.
pub(super) fn hub(
    store: &Arc<Store>,
    relay: Arc<dyn HelperRelay>,
    clock: &Arc<dyn Clock>,
) -> Arc<HelperTrustHub> {
    Arc::new(HelperTrustHub::new(
        store.clone(),
        store.clone(),
        relay,
        clock.clone(),
    ))
}

/// The helper trust use cases.
pub(super) fn service(
    hub: &Arc<HelperTrustHub>,
    store: &Arc<Store>,
    auth: &Arc<AuthState>,
    clock: &Arc<dyn Clock>,
) -> HelperTrust {
    HelperTrust::new(HelperTrustDeps {
        hub: hub.clone(),
        hosts: store.clone(),
        auth: auth.clone(),
        clock: clock.clone(),
    })
}

/// Asks a host's helper when the host authenticates.
pub(super) fn on_hello(hub: &Arc<HelperTrustHub>) -> Arc<dyn HostReconciler> {
    Arc::new(AttestOnEvent::new(hub.clone()))
}

/// Asks a host's helper when its agent reports new runtime status.
pub(super) fn on_status(server: &AgentServer, hub: &Arc<HelperTrustHub>) {
    server.set_status_listener(Arc::new(AttestOnEvent::new(hub.clone())));
}

/// Starts the fifteen-minute pass. It ends with `shutdown`; nothing waits on it.
pub(super) fn schedule(hub: Arc<HelperTrustHub>, store: &Arc<Store>, shutdown: CancellationToken) {
    drop(spawn_schedule(hub, store.clone(), shutdown));
}
