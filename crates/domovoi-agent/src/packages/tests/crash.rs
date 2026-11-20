//! Crash windows and boot recovery (Linux). A crash is rehearsed with an injected fault that
//! stops the operation without cleanup; reopening the service is the restart.

use domovoi_core::package::{PackageFailureCode as Code, PackageName, PackageVersion};
use domovoi_testkit::{BuiltPackage, PackageFixture};
use uuid::Uuid;

use super::env::{Env, install, request};
use super::support::code_of;
use crate::fs_atomic::DurableFaults;
use crate::packages::registry::Registry;
use crate::packages::{InstallStep, PackageError, PackageService, UninstallRequest};

fn pkg(version: &str) -> PackageFixture {
    PackageFixture::new("pkg-a", version)
        .unwrap()
        .skill("alpha")
}

fn registered(env: &Env) -> Option<String> {
    Registry::read(&env.packages().join(".packages.toml"))
        .unwrap()
        .get(&PackageName::parse("pkg-a").unwrap())
        .map(|entry| entry.version.to_string())
}

fn crash_at(service: &PackageService, step: InstallStep) {
    service.faults.lock().unwrap().crash_after = Some(step);
}

/// A service with `v1` installed, plus a signed `v2` ready to install.
fn with_v1() -> (Env, BuiltPackage, BuiltPackage) {
    let env = Env::new();
    let one = pkg("1.0.0").build();
    let two = pkg("2.0.0").sign_with(&one.signing_key).build();
    let service = env.open(&Env::section(&[&one]));
    install(&service, &one, "pkg-a", "1.0.0").unwrap();
    (env, one, two)
}

#[test]
fn crash_after_each_install_step_recovers() {
    let steps = [
        (InstallStep::AfterAudit, false),
        (InstallStep::AfterExtract, false),
        (InstallStep::AfterRename, false),
        (InstallStep::AfterRegistry, true),
        (InstallStep::AfterOldRemoved, true),
    ];
    for (step, committed) in steps {
        let (env, one, two) = with_v1();
        let section = Env::section(&[&one]);
        let service = env.open(&section);
        crash_at(&service, step);
        let req = request(&service, &two, "pkg-a", "2.0.0");
        let error = service.install(Uuid::now_v7(), req).unwrap_err();
        assert!(matches!(error, PackageError::InjectedCrash), "{step:?}");
        drop(service);

        let service = env.open(&section);
        let (kept, gone) = if committed {
            ("2.0.0", "1.0.0")
        } else {
            ("1.0.0", "2.0.0")
        };
        assert_eq!(registered(&env).as_deref(), Some(kept), "{step:?}");
        assert!(env.packages().join("pkg-a").join(kept).is_dir(), "{step:?}");
        assert!(
            !env.packages().join("pkg-a").join(gone).exists(),
            "{step:?}"
        );
        let staged: Vec<_> = std::fs::read_dir(env.packages().join(".staging"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(staged.len(), 1, "only transfers/ remains: {step:?}");

        let retry = install(&service, &two, "pkg-a", "2.0.0").unwrap();
        assert_eq!(retry.already_installed, committed, "{step:?}");
        assert_eq!(registered(&env).as_deref(), Some("2.0.0"));
        assert!(!env.packages().join("pkg-a/1.0.0").exists());
    }
}

#[test]
fn crash_during_uninstall_recovers() {
    for (step, removed_by_retry) in [
        (InstallStep::BeforeUninstallRegistry, true),
        (InstallStep::AfterUninstallRegistry, false),
    ] {
        let (env, one, _two) = with_v1();
        let section = Env::section(&[&one]);
        let service = env.open(&section);
        crash_at(&service, step);
        let request = UninstallRequest {
            package: PackageName::parse("pkg-a").unwrap(),
            version: PackageVersion::parse("1.0.0").unwrap(),
        };
        let error = service.uninstall(request.clone()).unwrap_err();
        assert!(matches!(error, PackageError::InjectedCrash));
        drop(service);

        let service = env.open(&section);
        if removed_by_retry {
            assert_eq!(registered(&env).as_deref(), Some("1.0.0"));
        } else {
            assert_eq!(registered(&env), None);
            assert!(!env.packages().join("pkg-a").exists());
        }
        assert_eq!(
            service.uninstall(request).unwrap().removed,
            removed_by_retry
        );
    }
}

#[test]
fn registry_write_failure_rolls_back_the_placed_tree() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    service.faults.lock().unwrap().durable = DurableFaults {
        rename: true,
        ..DurableFaults::default()
    };
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::Storage
    );
    assert!(!env.packages().join("pkg-a/1.0.0").exists());
    assert_eq!(registered(&env), None);
}

#[test]
fn indeterminate_registry_write_is_resolved_by_rereading() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    service.faults.lock().unwrap().durable = DurableFaults {
        dir_sync: true,
        ..DurableFaults::default()
    };
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    assert_eq!(registered(&env).as_deref(), Some("1.0.0"));
    assert!(env.packages().join("pkg-a/1.0.0").is_dir());
}

#[test]
fn corrupt_registry_fences_and_sweeps_nothing() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    drop(service);
    std::fs::write(env.packages().join(".packages.toml"), b"format = 9\n").unwrap();
    let service = env.open(&Env::section(&[&built]));
    assert_eq!(code_of(service.list()), Code::StoreCorrupt);
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::StoreCorrupt
    );
    assert!(env.packages().join("pkg-a/1.0.0/package.toml").is_file());
}

#[test]
fn unregistered_versions_are_swept_but_foreign_files_are_not() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let section = Env::section(&[&built]);
    drop(env.open(&section));
    let root = env.packages();
    std::fs::create_dir_all(root.join("pkg-x/1.0.0")).unwrap();
    std::fs::write(root.join("pkg-x/notes.txt"), b"mine").unwrap();
    std::fs::create_dir_all(root.join("pkg-y/1.0.0")).unwrap();
    std::fs::write(root.join("README"), b"mine").unwrap();
    std::fs::create_dir_all(root.join(".hidden/1.0.0")).unwrap();
    std::fs::create_dir_all(root.join("Bad Name/1.0.0")).unwrap();
    drop(env.open(&section));
    assert!(!root.join("pkg-x/1.0.0").exists());
    assert!(root.join("pkg-x/notes.txt").is_file());
    assert!(!root.join("pkg-y").exists());
    assert!(root.join("README").is_file());
    assert!(root.join(".hidden/1.0.0").is_dir());
    assert!(root.join("Bad Name/1.0.0").is_dir());
}

#[test]
fn staging_is_wiped_at_start() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let section = Env::section(&[&built]);
    let service = env.open(&section);
    let req = request(&service, &built, "pkg-a", "1.0.0");
    let leftover = env
        .packages()
        .join(".staging")
        .join(Uuid::now_v7().to_string());
    std::fs::create_dir_all(leftover.join("tree")).unwrap();
    drop(service);
    let service = env.open(&section);
    assert!(!leftover.exists());
    assert_eq!(service.staging().unwrap().staged_count(), 0);
    assert_eq!(
        code_of(service.install(Uuid::now_v7(), req)),
        Code::TransferUnknown
    );
}
