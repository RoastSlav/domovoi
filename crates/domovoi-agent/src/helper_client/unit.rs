//! The unit operations on the helper socket: the two typed calls, and nothing else.

use domovoi_core::helper::{HelperRequest, HelperResponse};
use domovoi_core::ports::Clock;
use domovoi_core::unit::UnitCall;

use super::{GroupsInfo, HelperClient, HelperClientError, HelperRelay};

impl HelperClient {
    /// Sends one unit call and returns the helper's answer as it came.
    pub async fn unit_call(
        &self,
        call: &UnitCall,
        clock: &dyn Clock,
    ) -> Result<HelperResponse, HelperClientError> {
        let request = match call {
            UnitCall::Identity { name } => HelperRequest::UnitIdentity { name: name.clone() },
            UnitCall::Restart { id, identity_hash } => HelperRequest::UnitRestart {
                id: id.clone(),
                identity_hash: *identity_hash,
            },
        };
        self.exchange(request, clock).await
    }
}

impl HelperRelay {
    /// Which groups the helper serves and what its platform allows, for what it offers.
    pub async fn groups(&self, clock: &dyn Clock) -> Result<GroupsInfo, HelperClientError> {
        self.client.groups(clock).await
    }

    /// Sends one unit call. The relay can send these two calls and nothing else of a unit.
    pub async fn unit(
        &self,
        call: &UnitCall,
        clock: &dyn Clock,
    ) -> Result<HelperResponse, HelperClientError> {
        self.client.unit_call(call, clock).await
    }
}
