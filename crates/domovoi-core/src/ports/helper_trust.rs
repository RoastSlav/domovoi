//! Read access to what the hub holds about a host's helper.

use async_trait::async_trait;

use crate::attest::TrustView;
use crate::id::HostId;
use crate::ports::repos::StoreError;

/// Reads the pin, the live session's key and the latest attestation outcome of a host.
///
/// Implemented by the hub over the store and the live session. The attested coverage check and
/// the containment decision read through it and never write.
#[async_trait]
pub trait HelperTrustRead: Send + Sync {
    /// The trust view of `host`: its pin, the key its session reported, the latest outcome and
    /// the stored verified state. All fields are empty for a host never seen.
    async fn view(&self, host: HostId) -> Result<TrustView, StoreError>;
}
