//! Coverage decided from the helper's verified attestation.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::{Cidr, SafetyIdentity, TrustView, confirmation_current, covered, gate};
use domovoi_core::knowledge::{CoverageEvidence, CoverageVerdict, UnverifiableReason};
use domovoi_core::ports::{Clock, HelperTrustRead, ManagementCoverage};
use jiff::Timestamp;

/// Coverage backed by the helper attestation: a path is covered only when the host's latest
/// attestation is verified, fresh and its effective set contains the path.
///
/// Nothing an agent reports about its own interfaces or routes is read here: the verified
/// state comes from a report the hub verified against the owner's pinned key.
pub struct AttestedCoverage {
    trust: Arc<dyn HelperTrustRead>,
    clock: Arc<dyn Clock>,
}

impl AttestedCoverage {
    /// Coverage over `trust`, with freshness measured by `clock`.
    #[must_use]
    pub fn new(trust: Arc<dyn HelperTrustRead>, clock: Arc<dyn Clock>) -> Self {
        Self { trust, clock }
    }

    async fn view(&self, host: HostId) -> Result<TrustView, UnverifiableReason> {
        // A store failure cannot prove anything: it reads as no attestation.
        self.trust
            .view(host)
            .await
            .map_err(|_| UnverifiableReason::AttestationMissing)
    }
}

#[async_trait]
impl ManagementCoverage for AttestedCoverage {
    fn is_available(&self) -> bool {
        true
    }

    async fn verdict(&self, host: HostId, cidr: &str) -> CoverageVerdict {
        let Ok(cidr) = Cidr::parse_config(cidr) else {
            return CoverageVerdict::Uncovered;
        };
        let view = match self.view(host).await {
            Ok(view) => view,
            Err(reason) => return CoverageVerdict::Unverifiable { reason },
        };
        let state = match gate(self.clock.now(), &view) {
            Ok(state) => state,
            Err(reason) => {
                return CoverageVerdict::Unverifiable {
                    reason: reason.into(),
                };
            }
        };
        if !covered(state, &cidr) {
            return CoverageVerdict::Uncovered;
        }
        match CoverageEvidence::new(state.identity.canonical()) {
            Some(evidence) => CoverageVerdict::Covered { evidence },
            None => CoverageVerdict::Unverifiable {
                reason: UnverifiableReason::AttestationMissing,
            },
        }
    }

    async fn confirmation_current(
        &self,
        host: HostId,
        evidence: &CoverageEvidence,
        confirmed_at: Timestamp,
    ) -> Result<(), UnverifiableReason> {
        let stale = UnverifiableReason::ConfirmationStale;
        let stored = SafetyIdentity::parse_canonical(evidence.as_str()).map_err(|_| stale)?;
        let view = self.view(host).await?;
        let now = self.clock.now();
        let current = gate(now, &view).map_err(UnverifiableReason::from)?;
        confirmation_current(&stored, confirmed_at, current, now).map_err(UnverifiableReason::from)
    }
}
