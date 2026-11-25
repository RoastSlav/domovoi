//! The tier gate for the write group.

use domovoi_config::parse_helper;
use domovoi_core::helper::UnavailableReason;
use domovoi_core::isolation::{PlatformRecordState, PlatformTier};
use domovoi_helper::systemd::write_gate;

#[cfg(test)]
fn config(toml: &str) -> domovoi_config::HelperConfig {
    parse_helper(toml).expect("config").0
}

const WRITE_ON: &str = "groups = [\"systemd.read\", \"systemd.write\"]\n";

#[test]
fn tier_unavailable_reasons_map_to_the_platform_tokens() {
    let on = config(WRITE_ON);
    let ok = PlatformRecordState::Ok;
    assert_eq!(write_gate(&on, ok, PlatformTier::Supported), Ok(()));
    assert_eq!(
        write_gate(&on, ok, PlatformTier::Untested),
        Err(UnavailableReason::WriteGroupNeedsOptIn)
    );
    assert_eq!(
        write_gate(&on, ok, PlatformTier::Blocked),
        Err(UnavailableReason::PlatformBlocked)
    );
    for state in [PlatformRecordState::Missing, PlatformRecordState::Stale] {
        assert_eq!(
            write_gate(&on, state, PlatformTier::Supported),
            Err(UnavailableReason::PlatformBlocked),
            "no sound record, no write"
        );
    }
}

#[test]
fn the_per_host_opt_in_allows_an_untested_platform_and_nothing_more() {
    let opted = config(&format!(
        "{WRITE_ON}[platform]\nallow_write_groups_on_untested = true\n"
    ));
    let ok = PlatformRecordState::Ok;
    assert_eq!(write_gate(&opted, ok, PlatformTier::Untested), Ok(()));
    assert_eq!(
        write_gate(&opted, ok, PlatformTier::Blocked),
        Err(UnavailableReason::PlatformBlocked)
    );
}

#[test]
fn a_fresh_install_has_no_write_group() {
    let fresh = config("");
    assert_eq!(
        write_gate(&fresh, PlatformRecordState::Ok, PlatformTier::Supported),
        Err(UnavailableReason::GroupDisabled)
    );
}
