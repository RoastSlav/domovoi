//! Install, uninstall and list against a real temporary directory (Linux).

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::atomic::Ordering;

use domovoi_core::package::{
    ArchiveDigest, KeyId, KeyState, PackageFailureCode as Code, PackageName, PackageStatus,
    PackageVersion, PrivilegeBoundary, keys,
};
use domovoi_runner::SkillCatalog;
use domovoi_testkit::PackageFixture;
use uuid::Uuid;

use super::env::{Env, install, request};
use super::support::code_of;
use crate::packages::events::EventKind;
use crate::packages::registry::{Registry, RegistryEntry};
use crate::packages::{ExecutionDenied, InstallRequest, UninstallRequest};

fn pkg(version: &str) -> PackageFixture {
    PackageFixture::new("pkg-a", version)
        .unwrap()
        .skill("alpha")
}

/// A request whose transfers were never stored (a fenced service has no staging area).
fn unstaged() -> InstallRequest {
    InstallRequest {
        package: name("pkg-a"),
        version: version("1.0.0"),
        artifact_transfer: Uuid::now_v7(),
        signature_transfer: Uuid::now_v7(),
    }
}

fn name(text: &str) -> PackageName {
    PackageName::parse(text).unwrap()
}

fn version(text: &str) -> PackageVersion {
    PackageVersion::parse(text).unwrap()
}

fn mode(path: &std::path::Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

fn uninstall(text: &str) -> UninstallRequest {
    UninstallRequest {
        package: name("pkg-a"),
        version: version(text),
    }
}

#[test]
fn install_extracts_with_normalized_modes_and_records_signer() {
    let env = Env::new();
    let built = pkg("1.0.0")
        .file_with_mode("data/tool.bin", b"x", 0o4775)
        .file_with_mode("data/group.txt", b"z", 0o2666)
        .file_with_mode("data/shared.txt", b"y", 0o664)
        .build();
    let service = env.open(&Env::section(&[&built]));
    let outcome = install(&service, &built, "pkg-a", "1.0.0").unwrap();
    let dir = env.packages().join("pkg-a/1.0.0");
    assert_eq!(mode(&dir), 0o755);
    assert_eq!(mode(&dir.join("skills")), 0o755);
    assert_eq!(mode(&dir.join("package.toml")), 0o644);
    assert_eq!(mode(&dir.join("skills/alpha/run")), 0o755);
    // Owner-executable in the archive keeps 0755; setuid and setgid bits never survive.
    assert_eq!(mode(&dir.join("data/tool.bin")), 0o755);
    assert_eq!(mode(&dir.join("data/group.txt")), 0o644);
    assert_eq!(mode(&dir.join("data/shared.txt")), 0o644);
    let registry = Registry::read(&env.packages().join(".packages.toml")).unwrap();
    let entry = registry.get(&name("pkg-a")).unwrap();
    assert_eq!(entry.key_id, built.key_id);
    assert_eq!(entry.archive_sha256, built.archive_sha256);
    assert_eq!(entry.installed_at, outcome.installed_at);
    assert_eq!(mode(&env.packages().join(".packages.toml")), 0o600);
}

#[test]
fn install_never_enables() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    let outcome = install(&service, &built, "pkg-a", "1.0.0").unwrap();
    let outputs = outcome.outputs();
    assert_eq!(outputs[keys::OUT_ENABLED], "false");
    assert_eq!(outputs[keys::OUT_SKILLS], "alpha");
    let (owner, _) = SkillCatalog::load(&env.dirs.skills_dir);
    assert!(owner.summaries().is_empty());
}

#[test]
fn install_refused_when_allow_install_false() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let mut section = Env::section(&[&built]);
    section.allow_install = false;
    let service = env.open(&section);
    assert!(!service.surface().mutate);
    assert!(service.surface().list);
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::PackagesDisabled
    );
    assert_eq!(
        code_of(service.uninstall(uninstall("1.0.0"))),
        Code::PackagesDisabled
    );
}

#[test]
fn reinstall_same_archive_is_idempotent() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    let first = install(&service, &built, "pkg-a", "1.0.0").unwrap();
    let second = install(&service, &built, "pkg-a", "1.0.0").unwrap();
    assert!(!first.already_installed);
    assert!(second.already_installed);
    assert!(!second.signer_updated);
    assert_eq!(second.installed_at, first.installed_at);
}

#[test]
fn same_version_other_digest_is_version_conflict() {
    let env = Env::new();
    let first = pkg("1.0.0").build();
    let second = pkg("1.0.0")
        .file("extra.txt", b"other")
        .sign_with(&first.signing_key)
        .build();
    let service = env.open(&Env::section(&[&first]));
    install(&service, &first, "pkg-a", "1.0.0").unwrap();
    assert_eq!(
        code_of(install(&service, &second, "pkg-a", "1.0.0")),
        Code::VersionConflict
    );
}

#[test]
fn update_replaces_old_version_and_removes_its_files() {
    let env = Env::new();
    let one = pkg("1.0.0").build();
    let two = pkg("2.0.0").sign_with(&one.signing_key).build();
    let service = env.open(&Env::section(&[&one]));
    install(&service, &one, "pkg-a", "1.0.0").unwrap();
    let outcome = install(&service, &two, "pkg-a", "2.0.0").unwrap();
    assert_eq!(outcome.replaced_version, Some(version("1.0.0")));
    assert!(!env.packages().join("pkg-a/1.0.0").exists());
    assert!(env.packages().join("pkg-a/2.0.0/package.toml").is_file());
    assert_eq!(service.list().unwrap().packages.len(), 1);
}

#[test]
fn rollback_requires_current_trust() {
    let env = Env::new();
    let a1 = pkg("1.0.0").build();
    let a2 = pkg("2.0.0").sign_with(&a1.signing_key).build();
    let b1 = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&a1, &b1]));
    install(&service, &a2, "pkg-a", "2.0.0").unwrap();
    drop(service);
    let service = env.open(&Env::section(&[&b1]));
    assert_eq!(
        code_of(install(&service, &a1, "pkg-a", "1.0.0")),
        Code::UntrustedKey
    );
    install(&service, &b1, "pkg-a", "1.0.0").unwrap();
    assert!(env.packages().join("pkg-a/1.0.0").is_dir());
    assert!(!env.packages().join("pkg-a/2.0.0").exists());
}

#[test]
fn resigned_package_with_trusted_key_updates_signer_without_reextracting() {
    let env = Env::new();
    let a = pkg("1.0.0").build();
    let b = pkg("1.0.0").build();
    assert_eq!(a.archive_sha256, b.archive_sha256);
    let service = env.open(&Env::section(&[&a, &b]));
    install(&service, &a, "pkg-a", "1.0.0").unwrap();
    let marker = env.packages().join("pkg-a/1.0.0/marker");
    std::fs::write(&marker, b"kept").unwrap();
    let outcome = install(&service, &b, "pkg-a", "1.0.0").unwrap();
    assert!(outcome.already_installed && outcome.signer_updated);
    assert!(marker.is_file());
    let registry = Registry::read(&env.packages().join(".packages.toml")).unwrap();
    assert_eq!(registry.get(&name("pkg-a")).unwrap().key_id, b.key_id);
}

#[test]
fn revoked_key_disables_installed_package() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    drop(service);
    let mut section = Env::section(&[&built]);
    section.revoked = vec![built.key_id.to_string()];
    let service = env.open(&section);
    let entry = service.list().unwrap().packages.remove(0);
    assert_eq!(entry.status, PackageStatus::DisabledUntrustedKey);
    assert_eq!(entry.key_state, KeyState::Revoked);
    assert!(matches!(
        service.resolve_for_execution(&name("pkg-a"), &version("1.0.0")),
        Err(ExecutionDenied::UntrustedKey {
            state: KeyState::Revoked,
            ..
        })
    ));
}

#[test]
fn removed_key_disables_installed_package() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let other = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    service
        .resolve_for_execution(&name("pkg-a"), &version("1.0.0"))
        .unwrap();
    drop(service);
    let service = env.open(&Env::section(&[&other]));
    let entry = service.list().unwrap().packages.remove(0);
    assert_eq!(entry.status, PackageStatus::DisabledUntrustedKey);
    assert_eq!(entry.key_state, KeyState::Unknown);
    assert!(matches!(
        service.resolve_for_execution(&name("pkg-a"), &version("1.0.0")),
        Err(ExecutionDenied::UntrustedKey { .. })
    ));
    assert!(
        env.events
            .events()
            .iter()
            .any(|event| event.kind == EventKind::KeyUntrusted)
    );
}

#[test]
fn uninstall_removes_files_and_registry_entry() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    let outcome = service.uninstall(uninstall("1.0.0")).unwrap();
    assert!(outcome.removed);
    assert!(!env.packages().join("pkg-a").exists());
    assert!(service.list().unwrap().packages.is_empty());
}

#[test]
fn uninstall_unknown_package_is_ok_with_removed_false() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    let outcome = service.uninstall(uninstall("1.0.0")).unwrap();
    assert!(!outcome.removed);
    assert_eq!(outcome.outputs()[keys::OUT_REMOVED], "false");
}

#[test]
fn uninstall_other_version_is_version_conflict() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    assert_eq!(
        code_of(service.uninstall(uninstall("2.0.0"))),
        Code::VersionConflict
    );
}

#[test]
fn uninstall_works_when_the_key_is_revoked() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    drop(service);
    let mut section = Env::section(&[&built]);
    section.revoked = vec![built.key_id.to_string()];
    let service = env.open(&section);
    assert!(service.uninstall(uninstall("1.0.0")).unwrap().removed);
}

#[test]
fn skill_name_conflict_with_owner_skill_is_rejected() {
    let env = Env::new();
    std::fs::create_dir_all(env.dirs.skills_dir.join("alpha")).unwrap();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::SkillConflict
    );
    assert!(!env.packages().join("pkg-a").exists());
}

#[test]
fn skill_name_conflict_with_other_package_is_rejected() {
    let env = Env::new();
    let first = pkg("1.0.0").build();
    let second = PackageFixture::new("pkg-b", "1.0.0")
        .unwrap()
        .skill("alpha")
        .sign_with(&first.signing_key)
        .build();
    let service = env.open(&Env::section(&[&first]));
    install(&service, &first, "pkg-a", "1.0.0").unwrap();
    assert_eq!(
        code_of(install(&service, &second, "pkg-b", "1.0.0")),
        Code::SkillConflict
    );
}

#[test]
fn run_as_in_package_skill_is_forbidden() {
    let env = Env::new();
    let manifest = "name = \"zeta\"\ndescription = \"d\"\nrisk = \"read\"\nentry = \"run\"\ntimeout = \"10s\"\nmax_output_bytes = 4096\nrun_as = \"root\"\n";
    let built = PackageFixture::new("pkg-a", "1.0.0")
        .unwrap()
        .file("skills/zeta/skill.toml", manifest.as_bytes())
        .file_with_mode("skills/zeta/run", b"#!/bin/sh\n", 0o755)
        .build();
    let service = env.open(&Env::section(&[&built]));
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::SkillRunAsForbidden
    );
}

#[test]
fn package_without_skills_is_rejected() {
    let env = Env::new();
    let built = PackageFixture::new("pkg-a", "1.0.0").unwrap().build();
    let service = env.open(&Env::section(&[&built]));
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::NoSkills
    );
}

/// Writes a registry of `count` packages named `pkg-<n>`, each with `skills` skills.
fn fill_registry(env: &Env, count: usize, skills: usize) {
    let mut registry = Registry::default();
    for index in 0..count {
        registry = registry.with(RegistryEntry {
            name: name(&format!("pkg-{index:02}")),
            version: version("1.0.0"),
            key_id: KeyId::parse("a1b2c3d4e5f60718").unwrap(),
            archive_sha256: ArchiveDigest::from_bytes([7; 32]),
            installed_at: "2026-11-18T10:00:00Z".parse().unwrap(),
            execution_id: Uuid::nil(),
            skills: (0..skills)
                .map(|n| format!("skill_{index:02}_{n:030}"))
                .collect(),
            tree_sha256: None,
        });
    }
    std::fs::create_dir_all(env.packages()).unwrap();
    std::fs::set_permissions(env.packages(), std::fs::Permissions::from_mode(0o755)).unwrap();
    registry
        .write(
            &env.packages().join(".packages.toml"),
            crate::fs_atomic::DurableFaults::default(),
        )
        .unwrap();
}

#[test]
fn too_many_packages_is_refused() {
    let env = Env::new();
    fill_registry(&env, 64, 1);
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::TooManyPackages
    );
}

#[test]
fn no_space_is_refused_before_extracting() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    env.space.0.store(1024, Ordering::SeqCst);
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::NoSpace
    );
    assert!(!env.packages().join("pkg-a").exists());
}

#[test]
fn packages_dir_not_owned_by_agent_or_group_writable_is_unsafe() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    std::fs::set_permissions(env.packages(), std::fs::Permissions::from_mode(0o775)).unwrap();
    assert_eq!(
        code_of(install(&service, &built, "pkg-a", "1.0.0")),
        Code::PackagesDirUnsafe
    );
    assert_eq!(code_of(service.list()), Code::PackagesDirUnsafe);
    let owner = std::fs::metadata(env.packages()).unwrap().uid();
    assert_eq!(owner, crate::packages::layout::effective_uid());
}

#[test]
fn packages_dir_symlink_is_refused() {
    let env = Env::new();
    let real = env.root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&real, env.packages()).unwrap();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    assert!(service.staging().is_none());
    assert_eq!(
        code_of(service.install(Uuid::now_v7(), unstaged())),
        Code::PackagesDirUnsafe
    );
}

#[test]
fn packages_dir_overlapping_protected_roots_is_refused() {
    let mut env = Env::new();
    let helper = env.root.path().join("helper");
    std::fs::create_dir(&helper).unwrap();
    env.dirs.protected = vec![helper.clone()];
    env.dirs.packages_dir = helper.join("packages");
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    assert_eq!(code_of(service.list()), Code::PackagesDirUnsafe);
    assert_eq!(
        code_of(service.install(Uuid::now_v7(), unstaged())),
        Code::PackagesDirUnsafe
    );
}

#[test]
fn root_agent_labels_privilege_boundary_not_in_effect() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let section = Env::section(&[&built]);
    let service =
        crate::packages::PackageService::open(&section, env.dirs.clone(), env.deps(true)).unwrap();
    let outcome = install(&service, &built, "pkg-a", "1.0.0").unwrap();
    assert_eq!(outcome.privilege_boundary, PrivilegeBoundary::NotInEffect);
    assert_eq!(
        outcome.outputs()[keys::OUT_PRIVILEGE_BOUNDARY],
        "not_in_effect"
    );
    assert_eq!(
        service.list().unwrap().outputs()[keys::OUT_PRIVILEGE_BOUNDARY],
        "not_in_effect"
    );
}

#[test]
fn list_reports_status_key_state_and_missing_files() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    let listed = service.list().unwrap();
    let entry = &listed.packages[0];
    assert_eq!(entry.status, PackageStatus::Active);
    assert_eq!(entry.key_state, KeyState::Trusted);
    assert!(!entry.executable);
    assert_eq!(
        entry.executable_reason.as_deref(),
        Some(crate::packages::enable::REASON_NOT_ENABLED)
    );
    std::fs::remove_dir_all(env.packages().join("pkg-a/1.0.0")).unwrap();
    assert_eq!(
        service.list().unwrap().packages[0].status,
        PackageStatus::MissingFiles
    );
    assert_eq!(
        service.resolve_for_execution(&name("pkg-a"), &version("1.0.0")),
        Err(ExecutionDenied::FilesMissing)
    );
}

#[test]
fn list_output_fits_its_limit_at_maximum_packages() {
    let env = Env::new();
    fill_registry(&env, 64, 32);
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    let outputs = service.list().unwrap().outputs();
    assert_eq!(outputs[keys::OUT_COUNT], "64");
    assert!(outputs[keys::OUT_PACKAGES].len() <= 262_144);
}

#[test]
fn unknown_transfers_are_refused() {
    let env = Env::new();
    let built = pkg("1.0.0").build();
    let service = env.open(&Env::section(&[&built]));
    let mut req = request(&service, &built, "pkg-a", "1.0.0");
    req.artifact_transfer = Uuid::now_v7();
    assert_eq!(
        code_of(service.install(Uuid::now_v7(), req)),
        Code::TransferUnknown
    );
}

fn boundary_of(euid: u32, groups: &[u32]) -> domovoi_core::isolation::BoundaryState {
    let account = domovoi_core::identity::UserEntry {
        name: domovoi_core::identity::AGENT_ACCOUNT.to_owned(),
        uid: 990,
        gid: 990,
        shell: "/usr/sbin/nologin".to_owned(),
    };
    domovoi_core::isolation::evaluate_boundary(euid, Some(&account), groups, None, None)
}

fn boundary_label(state: &domovoi_core::isolation::BoundaryState) -> PrivilegeBoundary {
    let env = Env::new();
    let mut deps = env.deps(false);
    deps.boundary_in_effect = state.in_effect();
    let section = Env::section(&[]);
    crate::packages::PackageService::open(&section, env.dirs.clone(), deps)
        .unwrap()
        .privilege_boundary()
}

#[test]
fn packages_boundary_is_in_effect_only_for_the_agent_account() {
    let state = boundary_of(990, &[990]);
    assert!(state.in_effect());
    assert_eq!(boundary_label(&state), PrivilegeBoundary::InEffect);
    let root = boundary_of(0, &[0]);
    assert_eq!(boundary_label(&root), PrivilegeBoundary::NotInEffect);
}

#[test]
fn packages_boundary_is_not_in_effect_for_a_nonroot_unexpected_user() {
    let other_user = boundary_of(1000, &[1000]);
    assert!(!other_user.in_effect());
    assert_eq!(boundary_label(&other_user), PrivilegeBoundary::NotInEffect);
    let extra_group = boundary_of(990, &[990, 4242]);
    assert_eq!(boundary_label(&extra_group), PrivilegeBoundary::NotInEffect);
}
