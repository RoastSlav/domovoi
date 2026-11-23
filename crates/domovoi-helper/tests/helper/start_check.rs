use domovoi_core::identity::{FaultKind, HostDirs, PathId, PathKind, RefusalToken};
use domovoi_core::ports::InspectError;
use domovoi_helper::start_check::run_start_checks;
use domovoi_testkit::FakeHostInspect;

use crate::common::{AGENT_ID, facts, healthy, healthy_with, root_dir, root_file};

fn dirs() -> HostDirs {
    HostDirs::default()
}

fn refusal(host: &FakeHostInspect) -> domovoi_helper::start_check::StartRefusal {
    run_start_checks(host, &dirs()).expect_err("the start check refuses")
}

// covers: helper_state_permissions
#[test]
fn helper_start_refuses_directory_mode_0755() {
    let d = dirs();
    let host = healthy(&d).path(&d.helper_dir, root_dir(0o755));
    let refused = refusal(&host);
    assert_eq!(refused.token, RefusalToken::HelperStatePermissions);
    assert_eq!(refused.faults.len(), 1);
    let fault = &refused.faults[0];
    assert_eq!(fault.id, PathId::HelperDir);
    assert_eq!(fault.why, FaultKind::Mode);
    assert_eq!(fault.found.as_ref().map(|f| f.mode), Some(0o755));
    assert_eq!(fault.expected.mode, 0o700);
}

#[test]
fn helper_start_refuses_key_with_group_or_world_bits() {
    let d = dirs();
    for mode in [0o640, 0o604, 0o666] {
        let host = healthy(&d).path(d.helper_dir.join("attest.key"), root_file(mode));
        let refused = refusal(&host);
        assert_eq!(refused.token, RefusalToken::HelperStatePermissions);
        assert_eq!(refused.faults[0].id, PathId::HelperKey);
        assert_eq!(refused.faults[0].why, FaultKind::Mode);
    }
}

#[test]
fn helper_start_refuses_symlinked_key() {
    let d = dirs();
    let mut link = root_file(0o600);
    link.symlink = true;
    let host = healthy(&d).path(d.helper_dir.join("attest.key"), link);
    let refused = refusal(&host);
    assert_eq!(refused.faults[0].id, PathId::HelperKey);
    assert_eq!(refused.faults[0].why, FaultKind::Symlink);
}

#[test]
fn helper_start_refuses_wrong_owner_and_wrong_type() {
    let d = dirs();
    let host = healthy(&d)
        .path(
            d.helper_dir.join("state.lock"),
            facts(PathKind::File, 0o600, 1000, 0),
        )
        .path(
            d.helper_dir.join("state"),
            facts(PathKind::Dir, 0o600, 0, 0),
        );
    let refused = refusal(&host);
    let why = |id| refused.faults.iter().find(|f| f.id == id).map(|f| f.why);
    assert_eq!(why(PathId::HelperLock), Some(FaultKind::Owner));
    assert_eq!(why(PathId::HelperState), Some(FaultKind::WrongType));
}

#[test]
fn helper_start_reports_every_offending_path_with_found_and_expected() {
    let d = dirs();
    let host = healthy(&d)
        .path(&d.helper_dir, root_dir(0o755))
        .path(d.helper_dir.join("state.lock"), root_file(0o644))
        .path(d.helper_dir.join("attest.key"), root_file(0o666));
    let refused = refusal(&host);
    let ids: Vec<PathId> = refused.faults.iter().map(|f| f.id).collect();
    assert_eq!(
        ids,
        [PathId::HelperDir, PathId::HelperLock, PathId::HelperKey]
    );
    for fault in &refused.faults {
        assert!(fault.found.is_some());
        assert_eq!(fault.expected.uid, 0);
        assert_eq!(fault.expected.gid, 0);
    }
}

// covers: helper_state_permissions
#[test]
fn helper_reports_permission_fault_not_corruption_for_unopenable_lock() {
    let d = dirs();
    let host = healthy(&d).open_result(
        d.helper_dir.join("state.lock"),
        Err(InspectError::PermissionDenied),
    );
    let refused = refusal(&host);
    assert_eq!(refused.token, RefusalToken::HelperStatePermissions);
    assert_eq!(refused.faults[0].id, PathId::HelperLock);
    assert_eq!(refused.faults[0].why, FaultKind::Unreadable);
}

#[test]
fn helper_start_accepts_absent_key_and_state_in_this_milestone() {
    let d = dirs();
    assert!(run_start_checks(&healthy(&d), &d).is_ok());
}

// covers: uninitialized
#[test]
fn helper_start_refuses_uninitialised_directory() {
    let d = dirs();
    let host = FakeHostInspect::new()
        .user(crate::common::agent_user(), &[AGENT_ID])
        .path(d.etc_dir.join("helper.toml"), root_file(0o600));
    let refused = refusal(&host);
    assert_eq!(refused.token, RefusalToken::Uninitialized);

    // The directory exists but `helper init` never created the lock.
    let host = FakeHostInspect::new()
        .user(crate::common::agent_user(), &[AGENT_ID])
        .path(&d.helper_dir, root_dir(0o700))
        .path(d.etc_dir.join("helper.toml"), root_file(0o600));
    assert_eq!(refusal(&host).token, RefusalToken::Uninitialized);
}

// covers: agent_account_missing
#[test]
fn helper_start_refuses_when_the_agent_account_is_missing() {
    let host = FakeHostInspect::new();
    assert_eq!(refusal(&host).token, RefusalToken::AgentAccountMissing);
}

// covers: helper_config_invalid
#[test]
fn helper_start_refuses_a_helper_toml_that_does_not_parse() {
    let d = dirs();
    for bad in [
        "bogus = 1\n",
        "groups = [\"docker.read\", \"made.up\"]\n",
        "groups = [\"docker.read\", \"docker.read\"]\n",
    ] {
        let host = healthy_with(&d, bad);
        let refused = refusal(&host);
        assert_eq!(refused.token, RefusalToken::HelperConfigInvalid, "{bad}");
        assert!(refused.faults.is_empty());
    }
}
