//! Typed configuration for the Domovoi hub and agent.
//!
//! Loads `hub.toml` and `agent.toml`, applies `DOMOVOI_*` environment
//! overrides, resolves secret indirection (`{ env = ".." }` or
//! `{ file = ".." }`) and reports every validation problem with its TOML path.
//! See `ARCHITECTURE.md` §14.
