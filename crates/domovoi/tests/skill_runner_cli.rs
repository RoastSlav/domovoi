//! The skill runner's and `agent dirs`' command lines, run as real processes.
#![cfg(unix)]

use assert_cmd::Command;
use predicates::str::contains;

fn domovoi() -> Command {
    let mut cmd = Command::cargo_bin("domovoi").unwrap();
    cmd.timeout(std::time::Duration::from_secs(60));
    cmd
}

fn agent_toml(dir: &std::path::Path, extra: &str) -> std::path::PathBuf {
    let path = dir.join("agent.toml");
    std::fs::write(
        &path,
        format!(
            "[agent]\nhost_name = \"box\"\nhub_url = \"wss://hub.example:7421\"\nstate_dir = '{}'\n{extra}[docker]\nenabled = false\n",
            dir.join("state").display()
        ),
    )
    .unwrap();
    path
}

#[test]
fn skill_runner_serve_refuses_relative_roots_with_exit_two() {
    domovoi()
        .args([
            "skill-runner",
            "serve",
            "--skills-dir",
            "skills",
            "--packages-dir",
            "/",
        ])
        .assert()
        .code(2)
        .stderr(contains("--skills-dir"));
}

#[test]
fn skill_runner_serve_refuses_a_tty_stdin() {
    // Standard input is not a socket here, so the adoption function refuses it.
    domovoi()
        .args([
            "skill-runner",
            "serve",
            "--skills-dir",
            "/",
            "--packages-dir",
            "/",
        ])
        .env_remove("LISTEN_FDS")
        .write_stdin("")
        .assert()
        .failure();
}

#[test]
fn agent_dirs_prints_defaults_without_a_file() {
    let dir = tempfile::tempdir().unwrap();
    domovoi()
        .args(["agent", "dirs", "--config"])
        .arg(dir.path().join("absent.toml"))
        .assert()
        .success()
        .stdout("skills_dir=/etc/domovoi/skills\npackages_dir=/var/lib/domovoi-packages\n");
}

#[test]
fn agent_dirs_prints_configured_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = agent_toml(
        dir.path(),
        "skills_dir = '/srv/domovoi/skills'\npackages_dir = '/srv/domovoi/packages'\n",
    );
    domovoi()
        .args(["agent", "dirs", "--config"])
        .arg(path)
        .assert()
        .success()
        .stdout("skills_dir=/srv/domovoi/skills\npackages_dir=/srv/domovoi/packages\n");
}

#[test]
fn agent_dirs_refuses_unit_unsafe_paths() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        "/srv/with space",
        "/srv/pct%x",
        "/srv/a|b",
        "/srv/a&b",
        "/run/skills",
        "/tmp/skills",
        "/home/me/skills",
        "/root/skills",
        "/proc/skills",
        "/sys/skills",
        "/dev/skills",
        "/etc/domovoi",
        "/etc",
        "/var/lib/domovoi-agent/skills",
        "/var/lib/domovoi-helper",
        "/srv/../skills",
        "/srv/skills/",
    ];
    for case in cases {
        let path = agent_toml(dir.path(), &format!("skills_dir = '{case}'\n"));
        domovoi()
            .args(["agent", "dirs", "--config"])
            .arg(&path)
            .assert()
            .failure()
            .stdout("");
    }
}
