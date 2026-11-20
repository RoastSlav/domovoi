//! The coverage hook as wired in this release.

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::knowledge::{CoverageVerdict, UnverifiableReason};
use domovoi_core::ports::ManagementCoverage;

/// Coverage that can never be verified: every check answers "cannot be verified", so no
/// management path can be confirmed. The helper attestation work replaces this wiring with an
/// attestation-backed implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableCoverage;

#[async_trait]
impl ManagementCoverage for UnavailableCoverage {
    fn is_available(&self) -> bool {
        false
    }

    async fn verdict(&self, _host: HostId, _cidr: &str) -> CoverageVerdict {
        CoverageVerdict::Unverifiable {
            reason: UnverifiableReason::NotAvailable,
        }
    }
}
