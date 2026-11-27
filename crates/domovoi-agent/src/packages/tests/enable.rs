//! Enabling a stored package: the default is off, enabling re-verifies the tree, a changed
//! tree or a removed package switches the skills off again, with the reason.

use domovoi_core::package::{PackageFailureCode as Code, PackageName, PackageVersion};
use domovoi_testkit::PackageFixture;

use super::env::{Env, install};
use super::support::code_of;
use crate::packages::enable::{REASON_NOT_ENABLED, tree_digest};
use crate::packages::{EnableRequest, UninstallRequest};

fn req() -> EnableRequest {
    EnableRequest {
        package: PackageName::parse("pkg-a").unwrap(),
        version: PackageVersion::parse("1.0.0").unwrap(),
    }
}

fn stored() -> (Env, crate::packages::PackageService) {
    let (env, service, _built) = stored_with_build();
    (env, service)
}

fn stored_with_build() -> (
    Env,
    crate::packages::PackageService,
    domovoi_testkit::BuiltPackage,
) {
    let env = Env::new();
    let built = PackageFixture::new("pkg-a", "1.0.0")
        .unwrap()
        .skill("alpha")
        .build();
    let service = env.open(&Env::section(&[&built]));
    install(&service, &built, "pkg-a", "1.0.0").unwrap();
    (env, service, built)
}

#[test]
fn a_stored_package_is_listed_disabled_with_the_reason() {
    let (_env, service) = stored();
    let view = service.skill_view(&[]);
    assert!(view.runnable.is_empty());
    assert_eq!(view.states.len(), 1);
    assert!(!view.states[0].enabled);
    assert_eq!(view.states[0].reason.as_deref(), Some(REASON_NOT_ENABLED));
    let entry = &service.list().unwrap().packages[0];
    assert!(!entry.executable);
}

#[test]
fn enable_turns_the_skills_on_and_disable_turns_them_off() {
    let (_env, service) = stored();
    let outcome = service.enable(req()).unwrap();
    assert!(outcome.enabled && outcome.changed);
    assert_eq!(outcome.tree_sha256.len(), 64);
    let view = service.skill_view(&[]);
    assert_eq!(view.runnable.len(), 1);
    assert!(view.states[0].enabled && view.states[0].reason.is_none());
    assert!(service.list().unwrap().packages[0].executable);
    assert!(!service.enable(req()).unwrap().changed, "idempotent");
    let off = service.disable(req()).unwrap();
    assert!(off.changed && !off.enabled);
    assert!(service.skill_view(&[]).runnable.is_empty());
    assert!(!service.disable(req()).unwrap().changed);
}

#[test]
fn enable_survives_a_restart_of_the_service() {
    let (env, service, built) = stored_with_build();
    service.enable(req()).unwrap();
    drop(service);
    let reopened = env.open(&Env::section(&[&built]));
    assert_eq!(reopened.skill_view(&[]).runnable.len(), 1);
}

#[test]
fn a_changed_tree_is_refused_and_switches_enabled_skills_off() {
    let (env, service) = stored();
    let dir = env.packages().join("pkg-a/1.0.0");
    let skill_file = std::fs::read_dir(dir.join("skills/alpha"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_file() && p.extension().is_none_or(|e| e != "toml"))
        .unwrap();
    // Changed before enabling: refused.
    let original = std::fs::read(&skill_file).unwrap();
    let mut changed = original.clone();
    changed.extend_from_slice(b"\n# tampered\n");
    std::fs::write(&skill_file, &changed).unwrap();
    assert_eq!(code_of(service.enable(req())), Code::TreeChanged);
    // Restored, enabled, then changed again: off with a reason.
    std::fs::write(&skill_file, &original).unwrap();
    service.enable(req()).unwrap();
    std::fs::write(&skill_file, &changed).unwrap();
    let view = service.skill_view(&[]);
    assert!(view.runnable.is_empty());
    assert!(
        view.states[0]
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("differ"))
    );
}

#[test]
fn an_owner_skill_with_the_same_name_wins() {
    let (_env, service) = stored();
    service.enable(req()).unwrap();
    let view = service.skill_view(&["alpha".to_owned()]);
    assert!(view.runnable.is_empty());
    assert!(!view.states[0].enabled);
}

#[test]
fn uninstall_forgets_the_enabled_state() {
    let (_env, service) = stored();
    service.enable(req()).unwrap();
    service
        .uninstall(UninstallRequest {
            package: req().package,
            version: req().version,
        })
        .unwrap();
    assert!(service.skill_view(&[]).states.is_empty());
}

#[test]
fn enable_of_an_unknown_package_or_version_is_refused() {
    let (_env, service) = stored();
    let mut other = req();
    other.version = PackageVersion::parse("2.0.0").unwrap();
    assert_eq!(code_of(service.enable(other)), Code::VersionConflict);
    let mut missing = req();
    missing.package = PackageName::parse("pkg-z").unwrap();
    assert_eq!(code_of(service.enable(missing)), Code::NotInstalled);
}

#[test]
fn the_tree_digest_changes_with_content_and_ignores_nothing() {
    let (env, _service) = stored();
    let dir = env.packages().join("pkg-a/1.0.0");
    let first = tree_digest(&dir).unwrap();
    assert_eq!(first, tree_digest(&dir).unwrap());
    std::fs::write(dir.join("extra.txt"), b"x").unwrap();
    assert_ne!(first, tree_digest(&dir).unwrap());
}
