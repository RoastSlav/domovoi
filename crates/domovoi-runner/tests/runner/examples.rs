//! The starter skills shipped in `examples/skills` load through the runner's real manifest
//! validation, so an example cannot rot. Linux only.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use domovoi_runner::SkillCatalog;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Copies the starter skills so the checkout's umask cannot leave a folder group-writable.
fn copied_starter_skills(into: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(workspace().join("examples/skills")).unwrap() {
        let from = entry.unwrap().path();
        let name = from.file_name().unwrap().to_string_lossy().into_owned();
        let to = into.join(&name);
        fs::create_dir_all(&to).unwrap();
        for file in fs::read_dir(&from).unwrap() {
            let file = file.unwrap().path();
            let target = to.join(file.file_name().unwrap());
            fs::copy(&file, &target).unwrap();
            let mode = if target.file_name().is_some_and(|n| n == "run") {
                0o755
            } else {
                0o644
            };
            fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
        }
        fs::set_permissions(&to, fs::Permissions::from_mode(0o755)).unwrap();
        names.push(name);
    }
    names.sort();
    names
}

#[test]
fn every_starter_skill_loads_and_is_documented() {
    let dir = tempfile::tempdir().unwrap();
    let names = copied_starter_skills(dir.path());
    assert!(!names.is_empty(), "examples/skills holds no skill");

    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(issues.is_empty(), "{issues:#?}");
    let loaded: Vec<String> = catalog
        .summaries()
        .into_iter()
        .map(|summary| summary.name.to_string())
        .collect();
    assert_eq!(loaded, names, "a starter skill did not load");

    let guide = fs::read_to_string(workspace().join("docs/book/src/guide/skills.md")).unwrap();
    for name in &names {
        assert!(
            workspace()
                .join("examples/skills")
                .join(name)
                .join("README.md")
                .is_file(),
            "{name} has no README.md"
        );
        assert!(
            guide.contains(&format!("`{name}`")),
            "{name} is missing from the skills guide"
        );
    }
}
