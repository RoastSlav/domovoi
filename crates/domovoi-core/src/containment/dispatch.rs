//! The closed set of calls the hub makes to the helper's `nft` operations, through an agent.
//!
//! No call carries a free argument: a phase, a canonical network, a TTL and the id of the block.
//! The script is rendered by the helper alone.

use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};

use super::keys;
use super::net::{Family, Net};
use crate::action::{AuthorizationKind, PlanHash};
use crate::attest::ExpectedSafety;
use crate::id::ExecutionId;

/// What one block is: which execution made it, which network, and how long it lasts once confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSpec {
    /// The execution id of the `apply`, which names the block in all three phases.
    pub block: ExecutionId,
    /// The network to block.
    pub net: Net,
    /// The TTL the block has after `confirm`.
    pub ttl: SignedDuration,
}

impl BlockSpec {
    /// The six arguments of the contract for `phase`: the same keys in every phase.
    ///
    /// `prefix_len_v4` is the target's own length when it is IPv4 and `32` otherwise;
    /// `prefix_len_v6` likewise with `128`, so a ceiling floor of the other family is always met.
    #[must_use]
    pub fn args(&self, phase: &str) -> BTreeMap<String, String> {
        let (v4, v6) = match self.net.family() {
            Family::V4 => (self.net.prefix_len(), 128),
            Family::V6 => (32, self.net.prefix_len()),
        };
        BTreeMap::from([
            (keys::ARG_PHASE.to_owned(), phase.to_owned()),
            (keys::ARG_SOURCE.to_owned(), self.net.canonical()),
            (keys::ARG_TTL_S.to_owned(), self.ttl.as_secs().to_string()),
            (keys::ARG_BLOCK.to_owned(), self.block.as_uuid().to_string()),
            (keys::ARG_PREFIX_LEN_V4.to_owned(), v4.to_string()),
            (keys::ARG_PREFIX_LEN_V6.to_owned(), v6.to_string()),
        ])
    }
}

/// What may be asked of an agent for containment. A closed type: no key, no raw script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainCall {
    /// Step one: a block with the kernel's 30 second timeout.
    Apply(BlockSpec),
    /// Step two, sent by the hub after the round trip: the block gets its real TTL.
    Confirm(BlockSpec),
    /// Remove the block.
    Rollback(BlockSpec),
    /// Read the helper's blocks, an owner-initiated refresh.
    List,
}

impl ContainCall {
    /// The block this call concerns, if any.
    #[must_use]
    pub fn spec(&self) -> Option<&BlockSpec> {
        match self {
            Self::Apply(spec) | Self::Confirm(spec) | Self::Rollback(spec) => Some(spec),
            Self::List => None,
        }
    }

    /// The contract this call runs.
    #[must_use]
    pub const fn contract_id(&self) -> &'static str {
        match self {
            Self::List => crate::capability::containment::LIST_BLOCKS_ID,
            _ => crate::capability::containment::BLOCK_SOURCE_ID,
        }
    }

    /// The phase spelling, `None` for a list.
    #[must_use]
    pub const fn phase(&self) -> Option<&'static str> {
        match self {
            Self::Apply(_) => Some(keys::PHASE_APPLY),
            Self::Confirm(_) => Some(keys::PHASE_CONFIRM),
            Self::Rollback(_) => Some(keys::PHASE_ROLLBACK),
            Self::List => None,
        }
    }

    /// The arguments on the wire: the six keys for a phase, none for a list.
    #[must_use]
    pub fn args(&self) -> BTreeMap<String, String> {
        match (self.spec(), self.phase()) {
            (Some(spec), Some(phase)) => spec.args(phase),
            _ => BTreeMap::new(),
        }
    }
}

/// Why a dispatch could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    /// A list carries no safety expectation and has its own constructor.
    #[error("a list is not a containment phase")]
    NotAPhase,
}

/// A call with what the agent needs to check it.
///
/// A phase always carries `expected_safety`; only [`ContainDispatch::list`] builds one without.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainDispatch {
    execution: ExecutionId,
    call: ContainCall,
    plan_hash: PlanHash,
    authorization: AuthorizationKind,
    expires_at: Timestamp,
    expected_safety: Option<ExpectedSafety>,
}

impl ContainDispatch {
    /// A phase of a block: `expected_safety` is required.
    ///
    /// # Errors
    /// [`DispatchError::NotAPhase`] for [`ContainCall::List`].
    pub fn phase(
        execution: ExecutionId,
        call: ContainCall,
        plan_hash: PlanHash,
        authorization: AuthorizationKind,
        expires_at: Timestamp,
        expected_safety: ExpectedSafety,
    ) -> Result<Self, DispatchError> {
        if matches!(call, ContainCall::List) {
            return Err(DispatchError::NotAPhase);
        }
        Ok(Self {
            execution,
            call,
            plan_hash,
            authorization,
            expires_at,
            expected_safety: Some(expected_safety),
        })
    }

    /// A read of the helper's blocks, always a person's request.
    #[must_use]
    pub fn list(execution: ExecutionId, plan_hash: PlanHash, expires_at: Timestamp) -> Self {
        Self {
            execution,
            call: ContainCall::List,
            plan_hash,
            authorization: AuthorizationKind::Human,
            expires_at,
            expected_safety: None,
        }
    }

    /// The execution, the idempotency key of the run.
    #[must_use]
    pub fn execution(&self) -> ExecutionId {
        self.execution
    }

    /// The call.
    #[must_use]
    pub fn call(&self) -> &ContainCall {
        &self.call
    }

    /// The plan hash the authorization covered.
    #[must_use]
    pub fn plan_hash(&self) -> PlanHash {
        self.plan_hash
    }

    /// Who authorized: a person or a policy permit.
    #[must_use]
    pub fn authorization(&self) -> AuthorizationKind {
        self.authorization
    }

    /// When the authorization stops being valid.
    #[must_use]
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    /// The safety the caller expects; `None` only for a list.
    #[must_use]
    pub fn expected_safety(&self) -> Option<&ExpectedSafety> {
        self.expected_safety.as_ref()
    }
}

/// What a containment operation answered: named string outputs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContainOutputs(pub BTreeMap<String, String>);

impl ContainOutputs {
    /// The output named `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}
