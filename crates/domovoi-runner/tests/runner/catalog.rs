//! Loading skill folders and the rules they must pass. Linux only.

use std::collections::BTreeMap;

use domovoi_core::{SkillName, ValidatedArgs};
use domovoi_runner::{RunError, SkillCatalog};
use domovoi_testkit::FakeProcessExecutor;

use crate::helpers::{canonical, chmod, install_skill};

fn only_issue(issues: &[domovoi_runner::CatalogIssue], needle: &str) {
    assert_eq!(issues.len(), 1, "{issues:#?}");
    assert!(issues[0].message.contains(needle), "{}", issues[0].message);
}

fn name(n: &str) -> SkillName {
    SkillName::parse(n).unwrap()
}

#[test]
fn valid_skills_load_with_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(issues.is_empty(), "{issues:#?}");
    let skill = catalog.get(&name("echo_args")).unwrap();
    assert_eq!(skill.manifest_hash(), skill.manifest().manifest_hash());
    assert_eq!(skill.dir(), canonical(&folder));
    assert_eq!(skill.entry_path(), canonical(&folder.join("run")));
    let summaries = catalog.summaries();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].manifest_hash, skill.manifest_hash());
}

#[test]
fn entry_escaping_skill_dir_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    std::fs::copy(folder.join("run"), dir.path().join("outside")).unwrap();
    chmod(&dir.path().join("outside"), 0o755);
    let manifest = std::fs::read_to_string(folder.join("skill.toml")).unwrap();
    std::fs::write(
        folder.join("skill.toml"),
        manifest.replace("entry = \"run\"", "entry = \"../outside\""),
    )
    .unwrap();
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(catalog.get(&name("echo_args")).is_none());
    assert_eq!(issues.len(), 1, "{issues:#?}");

    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    std::fs::remove_file(folder.join("run")).unwrap();
    let target = ["/bin/true", "/usr/bin/true"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap();
    std::os::unix::fs::symlink(target, folder.join("run")).unwrap();
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(catalog.get(&name("echo_args")).is_none());
    only_issue(&issues, "outside the skill folder");
}

#[test]
fn world_writable_entry_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    chmod(&folder.join("run"), 0o777);
    let (_, issues) = SkillCatalog::load(dir.path());
    only_issue(&issues, "writable by group or others");

    chmod(&folder.join("run"), 0o775);
    let (_, issues) = SkillCatalog::load(dir.path());
    only_issue(&issues, "writable by group or others");
}

#[test]
fn world_writable_folder_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    chmod(&folder, 0o777);
    let (_, issues) = SkillCatalog::load(dir.path());
    only_issue(&issues, "world-writable");
}

#[test]
fn non_executable_entry_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    chmod(&folder.join("run"), 0o644);
    let (_, issues) = SkillCatalog::load(dir.path());
    only_issue(&issues, "not executable");
}

#[test]
fn folder_name_mismatch_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    install_skill(dir.path(), "echo_args", "other_name");
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(catalog.summaries().is_empty());
    only_issue(&issues, "folder is named `other_name`");
}

#[test]
fn invalid_manifest_is_reported_and_skipped() {
    let dir = tempfile::tempdir().unwrap();
    install_skill(dir.path(), "echo_args", "echo_args");
    install_skill(dir.path(), "broken", "broken");
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert_eq!(catalog.summaries().len(), 1);
    assert_eq!(issues.len(), 2, "{issues:#?}");
    assert!(issues[0].skill_dir.ends_with("broken"));
    assert!(
        issues[0].message.contains("invalid skill.toml"),
        "{}",
        issues[0]
    );
    assert!(issues[1].skill_dir.ends_with("empty"));
    assert!(issues[1].message.contains("cannot read skill.toml"));
}

#[test]
fn missing_directory_is_one_issue() {
    let (catalog, issues) = SkillCatalog::load(std::path::Path::new("/nonexistent/skills"));
    assert!(catalog.summaries().is_empty());
    only_issue(&issues, "cannot read the skills directory");
}

#[test]
fn reload_picks_up_changes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, _) = SkillCatalog::load(dir.path());
    assert!(catalog.summaries().is_empty());
    install_skill(dir.path(), "echo_args", "echo_args");
    assert!(catalog.reload().is_empty());
    assert_eq!(catalog.summaries().len(), 1);
}

#[tokio::test]
async fn entry_swapped_after_load_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    let (catalog, _) = SkillCatalog::load(dir.path());
    let skill = catalog.get(&name("echo_args")).unwrap().clone();
    let args: ValidatedArgs = skill
        .manifest()
        .validate_args(&BTreeMap::from([("first".to_owned(), "x".to_owned())]))
        .unwrap();
    let executor = FakeProcessExecutor::new();

    chmod(&folder.join("run"), 0o777);
    assert!(matches!(
        catalog.run_skill(&skill, &args, &executor).await,
        Err(RunError::EntryChanged(m)) if m.contains("writable")
    ));

    chmod(&folder.join("run"), 0o755);
    let (other, _) = SkillCatalog::load(tempfile::tempdir().unwrap().path());
    assert!(matches!(
        other.run_skill(&skill, &args, &executor).await,
        Err(RunError::UnknownSkill(_))
    ));
}
