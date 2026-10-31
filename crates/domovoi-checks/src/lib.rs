//! Built-in checks and network probes.
//!
//! Host checks (feature `host`) read files through the `HostFs` port and run
//! commands only through the `ProcessExecutor` port. Network probes (feature
//! `net`) are used by both the agent and the hub. See `ARCHITECTURE.md` §4 and
//! §7.2.
//!
//! - [`framework`]: the [`Check`](framework::Check) trait, its context and reports.
//! - [`commands`]: the closed set of fixed command templates.
//! - `hostfs` and `system` (feature `host`): the real host filesystem and the system
//!   checks (disk, memory, temperature, reboots, kernel errors, power).

pub mod commands;
pub mod framework;

#[cfg(feature = "host")]
pub mod hostfs;
#[cfg(feature = "host")]
pub mod system;
