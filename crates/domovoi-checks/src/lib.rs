//! Built-in checks and network probes.
//!
//! Host checks (feature `host`) read files through the `HostFs` port and run
//! commands only through the `ProcessExecutor` port. Network probes (feature
//! `net`) are used by both the agent and the hub. See `ARCHITECTURE.md` §4 and
//! §7.2.
