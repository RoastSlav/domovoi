//! The argument and output names of the unit capabilities, written once.

/// The unit argument of both contracts.
pub const ARG_UNIT: &str = "unit";
/// The identity hash argument of a restart.
pub const ARG_IDENTITY_HASH: &str = "identity_hash";

/// Output: whether the unit restarted.
pub const OUT_RESTARTED: &str = "restarted";
/// Output: the invocation id before the restart.
pub const OUT_INVOCATION_BEFORE: &str = "invocation_before";
/// Output: the invocation id after the restart.
pub const OUT_INVOCATION_AFTER: &str = "invocation_after";
/// Output: the main pid after the restart.
pub const OUT_MAIN_PID: &str = "main_pid";
/// Output: `verified` or `unverified`.
pub const OUT_VERIFICATION: &str = "verification";
/// Output: `true` or `false`, a post-hoc "changed on disk" warning.
pub const OUT_STALE_START: &str = "stale_start_suspected";
/// Output (agent-added): the host's platform tier.
pub const OUT_PLATFORM_TIER: &str = "platform_tier";
/// Output (agent-added): whether the owner opted in to write operations on this host.
pub const OUT_WRITE_OPT_IN: &str = "write_opt_in";
/// Output of `unit.identity`: the identity hash.
pub const OUT_IDENTITY_HASH: &str = "identity_hash";
/// Output of `unit.identity`: the canonical id.
pub const OUT_ID: &str = "id";
/// Output of `unit.identity`: the unit's names, as a JSON array.
pub const OUT_NAMES: &str = "names";
/// Output of `unit.identity`: the unit file state.
pub const OUT_UNIT_FILE_STATE: &str = "unit_file_state";
/// Output of `unit.identity`: the unit file, path and sha256.
pub const OUT_FRAGMENT: &str = "fragment";
/// Output of `unit.identity`: the drop-ins, a JSON array of path and sha256.
pub const OUT_DROP_INS: &str = "drop_ins";
/// Output of `unit.identity`: the loaded facts a card shows, as JSON.
pub const OUT_LOADED: &str = "loaded";
/// Output of `unit.identity`: the unit's `ActiveState`.
pub const OUT_ACTIVE_STATE: &str = "active_state";

/// `verification` value: the new process was checked and matches.
pub const VERIFIED: &str = "verified";
/// `verification` value: the new process could not be checked.
pub const UNVERIFIED: &str = "unverified";
