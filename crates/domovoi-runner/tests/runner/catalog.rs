//! Loading skill folders and the rules they must pass. Linux only.

use std::collections::BTreeMap;

use domovoi_core::{ScriptDigest, SkillName, ValidatedArgs};
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
    assert!(summaries[0].readme.is_none());
}

#[test]
fn entry_script_digest_is_hash_of_resolved_file_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    let bytes = std::fs::read(folder.join("run")).unwrap();
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(issues.is_empty(), "{issues:#?}");
    let skill = catalog.get(&name("echo_args")).unwrap();
    assert_eq!(skill.entry_script_digest(), ScriptDigest::of_bytes(&bytes));

    // A symlink hashes the resolved target's bytes, never the link text.
    let linked = tempfile::tempdir().unwrap();
    let folder = install_skill(linked.path(), "echo_args", "echo_args");
    let target = folder.join("real_run");
    std::fs::rename(folder.join("run"), &target).unwrap();
    std::os::unix::fs::symlink("real_run", folder.join("run")).unwrap();
    chmod(&folder.join("run"), 0o755);
    let target_bytes = std::fs::read(&target).unwrap();
    let (catalog, issues) = SkillCatalog::load(linked.path());
    assert!(issues.is_empty(), "{issues:#?}");
    let skill = catalog.get(&name("echo_args")).unwrap();
    assert_eq!(skill.entry_path(), canonical(&target));
    assert_eq!(
        skill.entry_script_digest(),
        ScriptDigest::of_bytes(&target_bytes)
    );
}

#[test]
fn entry_script_digest_is_present_in_every_loaded_skill_summary() {
    let dir = tempfile::tempdir().unwrap();
    install_skill(dir.path(), "echo_args", "echo_args");
    let second = install_skill(dir.path(), "echo_args", "second");
    let manifest = std::fs::read_to_string(second.join("skill.toml")).unwrap();
    std::fs::write(
        second.join("skill.toml"),
        manifest.replace("name = \"echo_args\"", "name = \"second\""),
    )
    .unwrap();
    std::fs::write(second.join("run"), "#!/bin/sh\necho second\n").unwrap();
    chmod(&second.join("run"), 0o755);
    let (catalog, _) = SkillCatalog::load(dir.path());
    let summaries = catalog.summaries();
    assert_eq!(summaries.len(), 2);
    for summary in &summaries {
        let skill = catalog.get(&summary.name).unwrap();
        assert_eq!(
            summary.entry_script_digest,
            ScriptDigest::of_bytes(&std::fs::read(skill.entry_path()).unwrap())
        );
    }
    assert_ne!(
        summaries[0].entry_script_digest,
        summaries[1].entry_script_digest
    );
}

#[test]
fn unreadable_entry_keeps_the_skill_out_of_the_catalogue() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    // Executable but not readable: the entry check passes, the digest cannot.
    chmod(&folder.join("run"), 0o111);
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(catalog.get(&name("echo_args")).is_none());
    only_issue(&issues, "cannot read the resolved entry");
}

#[test]
fn folder_readme_is_reported_in_summaries() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    std::fs::write(
        folder.join("README.md"),
        "# Echo args\n\nEchoes its arguments.",
    )
    .unwrap();
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(issues.is_empty(), "{issues:#?}");
    let summaries = catalog.summaries();
    assert_eq!(summaries.len(), 1);
    assert_eq!(
        summaries[0].readme,
        Some("# Echo args\n\nEchoes its arguments.".to_owned())
    );
}

#[test]
fn oversized_readme_is_cut_on_a_character_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    std::fs::write(folder.join("README.md"), "€".repeat(7000)).unwrap();
    let (catalog, _) = SkillCatalog::load(dir.path());
    let summaries = catalog.summaries();
    assert_eq!(summaries.len(), 1);
    let text = summaries[0].readme.as_ref().unwrap();
    assert_eq!(
        text.len(),
        8190,
        "8 KiB limit, lowered to a character boundary"
    );
    assert_eq!(text.chars().count(), text.len() / 3);
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

#[tokio::test]
async fn entry_script_change_after_catalog_load_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let folder = install_skill(dir.path(), "echo_args", "echo_args");
    let (catalog, _) = SkillCatalog::load(dir.path());
    let skill = catalog.get(&name("echo_args")).unwrap().clone();
    let before = skill.entry_script_digest();
    let args: ValidatedArgs = skill
        .manifest()
        .validate_args(&BTreeMap::from([("first".to_owned(), "x".to_owned())]))
        .unwrap();
    let executor = FakeProcessExecutor::new();

    std::fs::write(folder.join("run"), "#!/bin/sh\nexit 7\n").unwrap();
    chmod(&folder.join("run"), 0o755);

    match catalog.run_skill(&skill, &args, &executor).await {
        Err(RunError::EntryChanged(message)) => {
            assert!(message.contains("bytes changed"), "{message}");
        }
        other => panic!("expected EntryChanged, got {other:?}"),
    }
    assert_eq!(
        skill.entry_script_digest(),
        before,
        "the stored digest never changes"
    );
}
