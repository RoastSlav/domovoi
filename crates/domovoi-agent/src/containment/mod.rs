//! The agent side of `firewall.block_source` and `firewall.list_blocks`.
//!
//! The root helper does the work and decides every safety question. This module parses the six
//! arguments, builds the typed helper call and maps the answer; the ordered request flow is in
//! `requests::firewall`, beside the unit one.

pub mod args;
pub mod backend;

/// The contract of one block's three phases.
pub const BLOCK_SOURCE: &str = domovoi_core::capability::containment::BLOCK_SOURCE_ID;
/// The contract of the owner-initiated refresh.
pub const LIST_BLOCKS: &str = domovoi_core::capability::containment::LIST_BLOCKS_ID;

/// Whether `contract` is one of the two containment contracts.
#[must_use]
pub fn is_containment_contract(contract: &str) -> bool {
    domovoi_core::containment::ops::is_containment_contract(contract)
}
