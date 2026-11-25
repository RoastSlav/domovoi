//! The two checks every read request passes before any work: is the group on, and does the
//! recorded platform allow reads.

use domovoi_config::HelperConfig;
use domovoi_core::helper::{HelperGroup, UnavailableReason};
use domovoi_core::identity::HostDirs;
use domovoi_core::isolation::CheckId;
use domovoi_core::ports::HostInspect;

use crate::serve::read_platform_record;

/// The helper's own isolation checks. A platform record that lists one of them as blocking means
/// the helper cannot vouch for its own confinement, so it reads nothing. Every other check
/// (the runner's, the state timing) concerns writes or the skill runner, not reads.
pub const READ_BLOCKING_CHECKS: [CheckId; 4] =
    [CheckId::St10, CheckId::St11, CheckId::St12, CheckId::St15];

/// Why `group` is not served right now, or `None` when the read may run.
///
/// Order: a group that is not enabled is `GroupDisabled`; `systemd.read` is `NotImplemented`; a
/// record for this kernel that blocks on a helper isolation check is `PlatformBlocked`. A missing
/// or stale record, an untested platform and a runner-blocked platform all allow reads.
#[must_use]
pub fn refusal(
    group: HelperGroup,
    config: &HelperConfig,
    inspect: &dyn HostInspect,
    dirs: &HostDirs,
) -> Option<UnavailableReason> {
    if !config.effective_groups().contains(&group) {
        return Some(UnavailableReason::GroupDisabled);
    }
    if group == HelperGroup::SystemdRead {
        return Some(UnavailableReason::NotImplemented);
    }
    let (_, record) = read_platform_record(inspect, dirs);
    let blocked = record.is_some_and(|record| {
        record
            .blocking
            .iter()
            .any(|check| READ_BLOCKING_CHECKS.contains(check))
    });
    blocked.then_some(UnavailableReason::PlatformBlocked)
}
