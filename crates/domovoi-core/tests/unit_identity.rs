//! The unit identity algorithm, run against small in-file fakes of the two read-only ports.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use domovoi_core::unit::{
    FileKind, FsError, LoadedProps, ProcessView, SystemdView, UnitFileState, UnitFs, UnitId,
    UnitName, UnitPolicy, UnitProps, UnitRefusal, UnitRuntime, UnsupportedReason, ViewError,
    compare_to_bound, resolve_identity,
};

const FRAGMENT: &str = "/etc/systemd/system/example.service";
const DROP_IN: &str = "/etc/systemd/system/example.service.d/10-local.conf";

#[cfg(test)]
#[derive(Clone)]
struct FakeSystemd {
    props: UnitProps,
    loaded: LoadedProps,
    fail_load: bool,
    fail_service: bool,
}

#[async_trait]
#[cfg(test)]
impl SystemdView for FakeSystemd {
    async fn load_unit(&self, _: &UnitName) -> Result<UnitProps, ViewError> {
        if self.fail_load {
            return Err(ViewError("busctl failed".into()));
        }
        Ok(self.props.clone())
    }
    async fn service_props(&self, _: &UnitId) -> Result<LoadedProps, ViewError> {
        if self.fail_service {
            return Err(ViewError("busctl failed".into()));
        }
        Ok(self.loaded.clone())
    }
    async fn runtime(&self, _: &UnitId) -> Result<UnitRuntime, ViewError> {
        Err(ViewError("unused".into()))
    }
    async fn process_view(&self, _: u32) -> Result<ProcessView, ViewError> {
        Err(ViewError("unused".into()))
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
struct FakeFs {
    kinds: BTreeMap<String, FileKind>,
    files: BTreeMap<String, Vec<u8>>,
    unreadable: BTreeSet<String>,
    disk_dropins: Vec<String>,
}

#[cfg(test)]
impl UnitFs for FakeFs {
    fn lstat(&self, path: &str) -> Result<FileKind, FsError> {
        self.kinds
            .get(path)
            .copied()
            .ok_or_else(|| FsError("missing".into()))
    }
    fn read_once(&self, path: &str) -> Result<Vec<u8>, FsError> {
        if self.unreadable.contains(path) {
            return Err(FsError("denied".into()));
        }
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| FsError("missing".into()))
    }
    fn enumerate_dropins(&self, _: &UnitId, _: &BTreeSet<String>) -> Result<Vec<String>, FsError> {
        Ok(self.disk_dropins.clone())
    }
}

#[cfg(test)]
fn props() -> UnitProps {
    UnitProps {
        id: "example.service".into(),
        names: BTreeSet::from(["example.service".to_owned()]),
        load_state: "loaded".into(),
        unit_file_state: "enabled".into(),
        transient: false,
        fragment_path: FRAGMENT.into(),
        source_path: String::new(),
        drop_in_paths: vec![DROP_IN.into()],
        need_daemon_reload: false,
        triggered_by: BTreeSet::new(),
        triggers: BTreeSet::new(),
    }
}

#[cfg(test)]
fn setup() -> (FakeSystemd, FakeFs, UnitPolicy) {
    let sys = FakeSystemd {
        props: props(),
        loaded: LoadedProps::default(),
        fail_load: false,
        fail_service: false,
    };
    let mut fs = FakeFs::default();
    for path in [FRAGMENT, DROP_IN] {
        fs.kinds.insert(path.into(), FileKind::Regular);
        fs.files.insert(path.into(), b"[Service]\n".to_vec());
    }
    fs.disk_dropins = vec![DROP_IN.into()];
    let policy = UnitPolicy {
        allow: BTreeSet::from([UnitId::parse("example.service").unwrap()]),
        protected: BTreeSet::from(["ssh.service".to_owned(), "ssh.socket".to_owned()]),
    };
    (sys, fs, policy)
}

#[cfg(test)]
fn name() -> UnitName {
    UnitName::parse("example.service").unwrap()
}

#[cfg(test)]
async fn run(sys: &FakeSystemd, fs: &FakeFs, policy: &UnitPolicy) -> Result<String, UnitRefusal> {
    resolve_identity(&name(), sys, fs, policy)
        .await
        .map(|i| i.hash().to_string())
}

#[cfg(test)]
fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    futures::executor::block_on(future)
}

#[test]
#[cfg(test)]
fn a_plain_unit_resolves_and_the_hash_is_stable() {
    let (sys, fs, policy) = setup();
    let first = block(run(&sys, &fs, &policy)).unwrap();
    assert_eq!(first, block(run(&sys, &fs, &policy)).unwrap());
    assert_eq!(first.len(), 64);
}

#[test]
#[cfg(test)]
fn bare_template_name_is_rejected() {
    assert!(UnitName::parse("worker@.service").is_err());
    assert!(UnitName::parse("worker@one.service").is_ok());
    assert!(UnitName::parse("-x.service").is_err());
    assert!(UnitName::parse("a*.service").is_err());
    assert!(UnitName::parse("example.socket").is_err());
}

#[test]
#[cfg(test)]
fn unit_identity_fits_target_identity_limit() {
    let long = format!("{}.service", "a".repeat(128 - ".service".len()));
    let id = UnitId::parse(&long).unwrap();
    assert_eq!(id.as_str().len(), 128);
    assert!(UnitId::parse(&format!("a{long}")).is_err());
}

#[test]
#[cfg(test)]
fn show_exit_zero_with_not_found_is_refused_target_not_found() {
    let (mut sys, fs, policy) = setup();
    sys.props.load_state = "not-found".into();
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetNotFound)
    );
}

#[test]
#[cfg(test)]
fn masked_unit_is_refused_by_load_state() {
    let (mut sys, fs, policy) = setup();
    sys.props.load_state = "masked".into();
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Masked))
    );
}

#[test]
#[cfg(test)]
fn enabled_runtime_unit_is_refused() {
    let (mut sys, fs, policy) = setup();
    sys.props.unit_file_state = "enabled-runtime".into();
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(
            UnsupportedReason::EnabledRuntime
        ))
    );
}

#[test]
#[cfg(test)]
fn transient_unit_is_refused() {
    let (mut sys, fs, policy) = setup();
    sys.props.transient = true;
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Transient))
    );
}

#[test]
#[cfg(test)]
fn protected_unit_matches_by_id_names_and_triggering_socket() {
    let (mut sys, fs, mut policy) = setup();
    policy.protected.insert("sshd.service".to_owned());
    sys.props.names.insert("sshd.service".to_owned());
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetProtected)
    );

    let (mut sys, fs, policy) = setup();
    sys.props.triggered_by.insert("ssh.socket".to_owned());
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetProtected)
    );
}

#[test]
#[cfg(test)]
fn unit_outside_the_allowlist_is_refused() {
    let (sys, fs, mut policy) = setup();
    policy.allow.clear();
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetNotAllowlisted)
    );
}

#[test]
#[cfg(test)]
fn need_daemon_reload_refuses_target_stale() {
    let (mut sys, fs, policy) = setup();
    sys.props.need_daemon_reload = true;
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetStale)
    );
}

#[test]
#[cfg(test)]
fn new_drop_in_without_reload_is_detected_by_disk_enumeration() {
    let (sys, mut fs, policy) = setup();
    fs.disk_dropins
        .push("/etc/systemd/system/example.service.d/99-new.conf".into());
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetStale)
    );
}

#[test]
#[cfg(test)]
fn a_disk_drop_in_in_a_forbidden_directory_is_refused_by_location() {
    let (sys, mut fs, policy) = setup();
    fs.disk_dropins
        .push("/run/systemd/system/example.service.d/x.conf".into());
    assert!(matches!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Location(
            _
        )))
    ));
}

#[test]
#[cfg(test)]
fn usr_local_lib_fragment_is_refused() {
    let (mut sys, mut fs, policy) = setup();
    let path = "/usr/local/lib/systemd/system/example.service";
    sys.props.fragment_path = path.into();
    fs.kinds.insert(path.into(), FileKind::Regular);
    assert!(matches!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Location(
            _
        )))
    ));
}

#[test]
#[cfg(test)]
fn symlinked_drop_in_is_refused() {
    let (sys, mut fs, policy) = setup();
    fs.kinds.insert(DROP_IN.into(), FileKind::Symlink);
    assert!(matches!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(UnsupportedReason::Symlink(
            _
        )))
    ));
}

#[test]
#[cfg(test)]
fn control_character_in_path_is_refused() {
    let (mut sys, fs, policy) = setup();
    sys.props.drop_in_paths = vec!["/etc/systemd/system/example.service.d/a\nb.conf".into()];
    assert_eq!(
        block(run(&sys, &fs, &policy)),
        Err(UnitRefusal::TargetUnsupported(
            UnsupportedReason::ControlCharacter
        ))
    );
}

#[test]
#[cfg(test)]
fn unreadable_drop_in_refuses_read_failed_never_empty_hash() {
    let (sys, mut fs, policy) = setup();
    fs.unreadable.insert(DROP_IN.into());
    assert_eq!(block(run(&sys, &fs, &policy)), Err(UnitRefusal::ReadFailed));
}

#[test]
#[cfg(test)]
fn a_manager_failure_is_read_failed_not_not_found() {
    let (mut sys, fs, policy) = setup();
    sys.fail_load = true;
    assert_eq!(block(run(&sys, &fs, &policy)), Err(UnitRefusal::ReadFailed));
    let (mut sys, fs, policy) = setup();
    sys.fail_service = true;
    assert_eq!(block(run(&sys, &fs, &policy)), Err(UnitRefusal::ReadFailed));
}

#[test]
#[cfg(test)]
fn d_bus_set_valued_properties_are_sorted_before_hashing() {
    let (mut a, fs, policy) = setup();
    let mut b = a.clone();
    a.loaded.requires = ["b.service", "a.service"].map(str::to_owned).into();
    b.loaded.requires = ["a.service", "b.service"].map(str::to_owned).into();
    assert_eq!(
        block(run(&a, &fs, &policy)).unwrap(),
        block(run(&b, &fs, &policy)).unwrap()
    );
}

#[test]
#[cfg(test)]
fn identity_hash_changes_on_content_and_precedence_but_not_on_mtime() {
    let (sys, fs, policy) = setup();
    let base = block(run(&sys, &fs, &policy)).unwrap();

    let mut edited = fs.clone();
    edited
        .files
        .insert(DROP_IN.into(), b"[Service]\n# c\n".to_vec());
    assert_ne!(base, block(run(&sys, &edited, &policy)).unwrap());

    // The content hash, not mtime, decides: identical bytes give the identical hash.
    let same = fs.clone();
    assert_eq!(base, block(run(&sys, &same, &policy)).unwrap());

    let mut reordered = sys.clone();
    let second = "/etc/systemd/system/example.service.d/20-second.conf".to_owned();
    let mut fs2 = fs.clone();
    fs2.kinds.insert(second.clone(), FileKind::Regular);
    fs2.files.insert(second.clone(), b"[Service]\n".to_vec());
    fs2.disk_dropins.push(second.clone());
    reordered.props.drop_in_paths = vec![DROP_IN.into(), second.clone()];
    let ordered = block(run(&reordered, &fs2, &policy)).unwrap();
    reordered.props.drop_in_paths = vec![second, DROP_IN.into()];
    assert_ne!(ordered, block(run(&reordered, &fs2, &policy)).unwrap());
}

#[test]
#[cfg(test)]
fn loaded_changes_are_target_changed_with_the_first_differing_field() {
    let (sys, fs, policy) = setup();
    let bound = block(resolve_identity(&name(), &sys, &fs, &policy)).unwrap();

    let mut changed = sys.clone();
    changed.loaded.user = "other".into();
    let now = block(resolve_identity(&name(), &changed, &fs, &policy)).unwrap();
    assert_eq!(
        compare_to_bound(&bound, &now),
        Err(UnitRefusal::TargetChanged {
            field: "loaded".into()
        })
    );

    let mut edited = fs.clone();
    edited.files.insert(DROP_IN.into(), b"changed".to_vec());
    let now = block(resolve_identity(&name(), &sys, &edited, &policy)).unwrap();
    assert_eq!(
        compare_to_bound(&bound, &now),
        Err(UnitRefusal::TargetChanged {
            field: format!("drop_in:{DROP_IN}")
        })
    );

    let mut other_state = sys.clone();
    other_state.props.unit_file_state = "disabled".into();
    let now = block(resolve_identity(&name(), &other_state, &fs, &policy)).unwrap();
    assert_eq!(bound.unit_file_state, UnitFileState::Enabled);
    assert_eq!(
        compare_to_bound(&bound, &now),
        Err(UnitRefusal::TargetChanged {
            field: "unit_file_state".into()
        })
    );
    assert_eq!(compare_to_bound(&bound, &bound), Ok(()));
}

#[test]
#[cfg(test)]
fn unit_refusal_tokens_round_trip() {
    let all = [
        UnitRefusal::TargetNotFound,
        UnitRefusal::TargetProtected,
        UnitRefusal::TargetNotAllowlisted,
        UnitRefusal::TargetStale,
        UnitRefusal::ReadFailed,
        UnitRefusal::PlatformUntested,
        UnitRefusal::PlatformBlocked,
        UnitRefusal::RestartBudgetExhausted,
        UnitRefusal::RestartFailed,
        UnitRefusal::StartLimitHit,
        UnitRefusal::NoRestartObserved,
        UnitRefusal::TargetChanged {
            field: "loaded".into(),
        },
        UnitRefusal::WrongStart {
            fields: vec!["environment".into(), "user".into()],
        },
        UnitRefusal::location("/run/x.service"),
        UnitRefusal::symlink("/etc/systemd/system/x.service"),
        UnitRefusal::TargetUnsupported(UnsupportedReason::Masked),
        UnitRefusal::TargetUnsupported(UnsupportedReason::EnabledRuntime),
    ];
    for refusal in all {
        let text = refusal.to_string();
        assert_eq!(
            UnitRefusal::from_message(&text),
            Some(refusal.clone()),
            "{text}"
        );
        assert!(!refusal.describe().is_empty());
    }
    assert_eq!(UnitRefusal::from_message("something_else"), None);
}
