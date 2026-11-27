//! The starter skills shipped in `examples/skills` load through the runner's real manifest
//! validation, so an example cannot rot. Linux only.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use domovoi_core::{AgentRead, HostNeed, Risk};
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

/// Runs a starter skill's `run` with fake programs first on `PATH`, so a hidden host view
/// (an unreadable journal, an unreachable service bus, a process list without pid 1) can
/// be reproduced on any machine.
#[allow(clippy::disallowed_methods)]
fn run_with_fakes(skill: &str, args: &[&str], fakes: &[(&str, &str)]) -> (i32, String, String) {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in fakes {
        let path = dir.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let script = workspace().join("examples/skills").join(skill).join("run");
    let output = std::process::Command::new("sh")
        .arg(script)
        .args(args)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn starter_read_skills_fail_loudly_when_the_host_view_is_hidden() {
    let blind_journal = [("journalctl", "echo 'No journal files were found.'; exit 1")];
    for (skill, args) in [
        ("log_summary", vec![]),
        ("journal_errors", vec!["nginx.service", "10"]),
        ("hardware_health", vec![]),
    ] {
        let (code, _, stderr) = run_with_fakes(skill, &args, &blind_journal);
        assert_eq!(code, 3, "{skill} exited {code}: {stderr}");
        assert!(stderr.contains("host view hidden"), "{skill}: {stderr}");
    }
    let (code, _, stderr) = run_with_fakes(
        "service_list",
        &[],
        &[("systemctl", "echo 'Failed to connect to bus'; exit 1")],
    );
    assert_eq!(code, 3, "{stderr}");
    assert!(stderr.contains("host view hidden"), "{stderr}");
    let (code, _, stderr) = run_with_fakes("top_processes", &[], &[("ps", "exit 1")]);
    assert_eq!(code, 3, "{stderr}");
    assert!(stderr.contains("pid 1 is not visible"), "{stderr}");
}

/// Behind the skill runner every starter read skill either reads only what the sandbox shows,
/// or names the built-in read that answers it; the one that can do neither says what it
/// needs, so the hub shows it unavailable.
#[test]
fn every_starter_read_skill_is_served_or_declares_what_is_hidden() {
    let dir = tempfile::tempdir().unwrap();
    copied_starter_skills(dir.path());
    let (catalog, _) = SkillCatalog::load(dir.path());
    let mut served = Vec::new();
    for summary in catalog.summaries() {
        if summary.risk != Risk::Read {
            continue;
        }
        let manifest = catalog.get(&summary.name).unwrap().manifest();
        served.push((
            summary.name.to_string(),
            manifest.agent_read,
            manifest.unanswered_needs(),
        ));
    }
    let own = |name: &str| (name.to_owned(), None, Vec::new());
    let agent = |name: &str, read| (name.to_owned(), Some(read), Vec::new());
    assert_eq!(
        served,
        [
            ("disk_usage_top".to_owned(), None, vec![HostNeed::Files]),
            agent("hardware_health", AgentRead::Hardware),
            agent("journal_errors", AgentRead::JournalErrors),
            agent("log_summary", AgentRead::LogSummary),
            own("power_status"),
            agent("service_list", AgentRead::Services),
            own("system_overview"),
            agent("top_processes", AgentRead::Processes),
        ]
    );
}

#[test]
fn journal_errors_says_when_there_are_none() {
    let quiet = [("journalctl", "exit 0")];
    let (code, stdout, _) = run_with_fakes("journal_errors", &["nginx.service", "10"], &quiet);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "no errors logged by nginx.service in the last 10 minutes\n"
    );
}
