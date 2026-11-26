//! The container-output parsers, re-exported from `domovoi-core` so the hub's inventory and the
//! engine's target resolver read one implementation.
//!
//! `docker_ps` on the agent prints a tab-separated table with a header and no id; the hub takes
//! only column 0. `docker_inspect` returns Docker's own document, whose top-level `Id` is the
//! full container id a plan binds. A short id is refused, never expanded.

pub use domovoi_core::capability::inventory::{
    ContainerRef, InventoryError, parse_inspect_id, parse_ps,
};
