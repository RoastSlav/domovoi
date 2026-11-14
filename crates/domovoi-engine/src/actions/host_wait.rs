//! Waiting for a host's agent before an approved action is dispatched.
//!
//! A hub that restarted finds approvals in the store whose host has not reconnected yet.
//! Whether that host is gone for good or was simply not heard from cannot be known at
//! start-up, so the action waits on the [`Clock`](domovoi_core::ports::Clock) for up to
//! [`ActionSettings::offline_wait`](super::ActionSettings::offline_wait), checking
//! availability every `presence_poll`. The wait happens before the catch-up pool is
//! entered, so actions waiting for many hosts never hold the runs that could proceed.

use domovoi_core::HostId;
use domovoi_core::ports::HostAvailability;
use jiff::Timestamp;

use super::Shared;

/// How a wait for the host ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostWait {
    /// The agent is connected now.
    Known,
    /// The hub rejected the agent's protocol; only upgrading the agent helps.
    Incompatible,
    /// The wait ran past `offline_wait` with the agent still gone.
    TimedOut,
}

/// Waits for `host`'s agent from `started`, up to `offline_wait` of the shared clock.
///
/// `started` is when this run read the action, not when it was approved: a restart must
/// give the agent its full wait, however long ago the approval was made.
pub(crate) async fn wait_for_host(shared: &Shared, host: HostId, started: Timestamp) -> HostWait {
    let deps = &shared.deps;
    let give_up = started
        .checked_add(shared.settings.offline_wait)
        .unwrap_or(started);
    loop {
        match deps.link.availability(host).await {
            HostAvailability::Known { .. } => return HostWait::Known,
            HostAvailability::Incompatible { .. } => return HostWait::Incompatible,
            HostAvailability::Unknown | HostAvailability::Absent { .. } => {}
        }
        let now = deps.clock.now();
        if now >= give_up {
            return HostWait::TimedOut;
        }
        let next = now
            .checked_add(shared.settings.presence_poll)
            .unwrap_or(give_up)
            .min(give_up);
        deps.clock.sleep_until(next).await;
    }
}
