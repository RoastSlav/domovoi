//! The `nft` group on the helper socket: the four typed firewall calls, and nothing else.

use domovoi_core::helper::{BlockSource, HelperRequest, HelperResponse};
use domovoi_core::ports::Clock;
use domovoi_proto::messages::ExpectedSafety;
use uuid::Uuid;

use super::{HelperClient, HelperClientError, HelperRelay};

/// What may be asked of the helper's `nft` group. A closed type: no script, no key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirewallCall {
    /// Step one of a block.
    Apply {
        /// The block id, the execution id of the apply.
        block: Uuid,
        /// The canonical network text.
        source: BlockSource,
        /// The TTL the block has after `confirm`.
        ttl_s: u32,
        /// What the approval bound; relayed as it came.
        expected_safety: Option<ExpectedSafety>,
    },
    /// Step two of a block.
    Confirm {
        /// The block id.
        block: Uuid,
        /// The canonical network text.
        source: BlockSource,
        /// The TTL, equal to the apply's.
        ttl_s: u32,
        /// What the approval bound; relayed as it came.
        expected_safety: Option<ExpectedSafety>,
    },
    /// Removes a block.
    Rollback {
        /// The block id.
        block: Uuid,
        /// The canonical network text.
        source: BlockSource,
        /// The TTL of the block.
        ttl_s: u32,
    },
    /// The blocks the helper holds.
    List,
}

impl FirewallCall {
    fn request(&self) -> HelperRequest {
        match self.clone() {
            Self::Apply {
                block,
                source,
                ttl_s,
                expected_safety,
            } => HelperRequest::FirewallApply {
                block,
                source,
                ttl_s,
                expected_safety,
            },
            Self::Confirm {
                block,
                source,
                ttl_s,
                expected_safety,
            } => HelperRequest::FirewallConfirm {
                block,
                source,
                ttl_s,
                expected_safety,
            },
            Self::Rollback {
                block,
                source,
                ttl_s,
            } => HelperRequest::FirewallRollback {
                block,
                source,
                ttl_s,
            },
            Self::List => HelperRequest::FirewallListBlocks,
        }
    }
}

impl HelperClient {
    /// Sends one firewall call and returns the helper's answer as it came.
    pub async fn firewall_call(
        &self,
        call: &FirewallCall,
        clock: &dyn Clock,
    ) -> Result<HelperResponse, HelperClientError> {
        self.exchange(call.request(), clock).await
    }
}

impl HelperRelay {
    /// Sends one firewall call. The relay can send these four and nothing else of the group.
    pub async fn firewall(
        &self,
        call: &FirewallCall,
        clock: &dyn Clock,
    ) -> Result<HelperResponse, HelperClientError> {
        self.client.firewall_call(call, clock).await
    }
}
