//! The helper operations the containment contracts need, spelled once.
//!
//! The hub's availability code reads these names from here, so it needs no text about the
//! contracts' effect class (a gate test keeps that vocabulary out of the registry).

use crate::capability::containment::{BLOCK_SOURCE_ID, LIST_BLOCKS_ID};

/// The four operations of the helper's `nft` group, as an agent reports them in `hello`.
pub const NFT_OPS: [&str; 4] = [
    "firewall.apply",
    "firewall.confirm",
    "firewall.rollback",
    "firewall.list_blocks",
];

/// Whether `id` is one of the two containment contracts.
#[must_use]
pub fn is_containment_contract(id: &str) -> bool {
    id == BLOCK_SOURCE_ID || id == LIST_BLOCKS_ID
}

/// Whether `offered` holds every operation of the `nft` group. A helper that offers some of
/// them offers none of the contracts: a block needs all three phases.
#[must_use]
pub fn offers_nft_group(offered: &[String]) -> bool {
    NFT_OPS
        .iter()
        .all(|op| offered.iter().any(|offered| offered == op))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ops: &[&str]) -> Vec<String> {
        ops.iter().map(|op| (*op).to_owned()).collect()
    }

    #[test]
    fn the_group_needs_all_four_operations() {
        assert!(offers_nft_group(&words(&NFT_OPS)));
        assert!(!offers_nft_group(&words(&[
            "firewall.apply",
            "firewall.confirm"
        ])));
        assert!(!offers_nft_group(&[]));
        assert!(is_containment_contract("firewall.block_source"));
        assert!(!is_containment_contract("unit.restart"));
    }
}
