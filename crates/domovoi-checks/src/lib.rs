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
//! - `containers`, `units`, `smart` and `security` (feature `host`): service checks.
//! - `net` (feature `net`): TCP, HTTP and gateway probes; they do not need `host`.
//! - `sources` (feature `sources`): pure parsers for webhook payloads from Alertmanager,
//!   Uptime Kuma and the generic format, shared by the hub and the agent's relay.

pub mod commands;
pub mod framework;

#[cfg(feature = "host")]
pub mod containers;
#[cfg(feature = "host")]
pub mod hostfs;
#[cfg(feature = "host")]
pub mod learn;
#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "host")]
pub mod security;
#[cfg(feature = "host")]
pub mod smart;
#[cfg(feature = "sources")]
pub mod sources;
#[cfg(feature = "host")]
pub mod system;
#[cfg(feature = "host")]
pub mod units;
