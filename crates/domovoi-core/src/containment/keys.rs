//! Every argument and output key of the containment contracts, spelled once.

/// Argument: `apply`, `confirm` or `rollback`.
pub const ARG_PHASE: &str = "phase";
/// Argument: the canonical network text.
pub const ARG_SOURCE: &str = "source";
/// Argument: the TTL the block has after `confirm`, in seconds.
pub const ARG_TTL_S: &str = "ttl_s";
/// Argument: the execution id of the apply that made the block.
pub const ARG_BLOCK: &str = "block";
/// Argument: the shortest IPv4 prefix the block may have, for the host ceiling.
pub const ARG_PREFIX_LEN_V4: &str = "prefix_len_v4";
/// Argument: the shortest IPv6 prefix the block may have, for the host ceiling.
pub const ARG_PREFIX_LEN_V6: &str = "prefix_len_v6";

/// All six argument keys, in schema order.
pub const ARG_KEYS: [&str; 6] = [
    ARG_PHASE,
    ARG_SOURCE,
    ARG_TTL_S,
    ARG_BLOCK,
    ARG_PREFIX_LEN_V4,
    ARG_PREFIX_LEN_V6,
];

/// Phase value of the first step.
pub const PHASE_APPLY: &str = "apply";
/// Phase value of the hub's confirmation.
pub const PHASE_CONFIRM: &str = "confirm";
/// Phase value of a removal.
pub const PHASE_ROLLBACK: &str = "rollback";

/// Output: the phase that ran.
pub const OUT_PHASE: &str = "phase";
/// Output: the block id.
pub const OUT_BLOCK: &str = "block";
/// Output: the canonical source.
pub const OUT_SOURCE: &str = "source";
/// Output: the nft set that holds the element.
pub const OUT_SET: &str = "set";
/// Output: when the helper applied the block.
pub const OUT_APPLIED_AT: &str = "applied_at";
/// Output: the kernel timeout of step one, in seconds.
pub const OUT_STEP1_TTL_S: &str = "step1_ttl_s";
/// Output: seconds left on the element.
pub const OUT_REMAINING_S: &str = "remaining_s";
/// Output: `ok`, `created` or `recreated`.
pub const OUT_TABLE_STATE: &str = "table_state";
/// Output: `hosts` or `hosts_and_prefixes`.
pub const OUT_LAYOUT: &str = "layout";
/// Output: the helper's safety generation.
pub const OUT_GENERATION: &str = "generation";
/// Output: `cut`, `n/a` or `may_persist`.
pub const OUT_EXISTING_FLOWS: &str = "existing_flows";
/// Output: `true` or `false`.
pub const OUT_BRIDGE_GAP: &str = "bridge_gap";
/// Output: blocks lost with the table.
pub const OUT_LOST_BLOCKS: &str = "lost_blocks";
/// Output: whether the chain matched its template.
pub const OUT_CHAIN_OK: &str = "chain_ok";
/// Output: the arrival counter evidence.
pub const OUT_ARRIVALS: &str = "arrivals";
/// Output: the leak self-test gate state.
pub const OUT_GATE: &str = "gate";
/// Output: the drill state.
pub const OUT_DRILLS: &str = "drills";
/// Output: the block list as JSON, from `firewall.list_blocks` only.
pub const OUT_BLOCKS: &str = "blocks";
/// Output: how many elements are active.
pub const OUT_ACTIVE: &str = "active";
/// Output: members with no journal entry.
pub const OUT_ORPHANS: &str = "orphans";
