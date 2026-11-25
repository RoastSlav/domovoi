//! The closed set of calls the hub makes to the helper's unit operations. No call carries a
//! free argument: a name, or an id with the identity hash it was approved against.

use std::collections::BTreeMap;

use jiff::Timestamp;

use super::name::{IdentityHash, UnitId, UnitName};
use crate::action::{AuthorizationKind, PlanHash};
use crate::id::ExecutionId;

/// One call to a unit operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitCall {
    /// Read the identity of the named unit.
    Identity {
        /// The unit as typed.
        name: UnitName,
    },
    /// Restart the unit with this canonical id, if it still has this identity.
    Restart {
        /// The canonical id the manager reports.
        id: UnitId,
        /// The identity hash that was approved.
        identity_hash: IdentityHash,
    },
}

/// A restart call with what the agent needs to check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitDispatch {
    /// The execution, the idempotency key of the run.
    pub execution: ExecutionId,
    /// The call.
    pub call: UnitCall,
    /// The plan hash the authorization covered.
    pub plan_hash: PlanHash,
    /// Who authorized: a person or a policy permit.
    pub authorization_kind: AuthorizationKind,
    /// When the authorization stops being valid.
    pub expires_at: Timestamp,
}

/// What a unit operation answered: named string outputs, at most 16 KiB.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitOutputs(pub BTreeMap<String, String>);

impl UnitOutputs {
    /// The output named `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}
