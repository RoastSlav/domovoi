//! Building the gate's view of a host from the store and the live session.

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::TrustView;
use domovoi_core::ports::{HelperTrustRead, StoreError, StoredTrust};

use super::HelperTrustHub;

impl HelperTrustHub {
    /// The stored trust rows of `host` with the key its live session relayed filled in. The
    /// relayed key is never trusted: the gate only compares it with the pin.
    ///
    /// # Errors
    /// A store failure.
    pub(crate) async fn stored_with_live(&self, host: HostId) -> Result<StoredTrust, StoreError> {
        let mut stored = self.repo.stored(host).await?;
        stored.view.received_key = self.relay.report(host).and_then(|report| report.public_key);
        Ok(stored)
    }
}

#[async_trait]
impl HelperTrustRead for HelperTrustHub {
    async fn view(&self, host: HostId) -> Result<TrustView, StoreError> {
        Ok(self.stored_with_live(host).await?.view)
    }
}
