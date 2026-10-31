//! systemd unit checks: failed units and watched units.
//!
//! Both run `systemctl` through the fixed templates in [`crate::commands`]. Targets are
//! unit names; states and restart counts are evidence.

mod failed;
mod watch;

pub use failed::{FailedUnit, FailedUnits, parse_failed_json, parse_failed_plain};
pub use watch::{UnitProps, WatchedUnits, parse_show};
