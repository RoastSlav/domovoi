//! The `domovoi` command line, run as a real process.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use assert_cmd::Command;
    use predicates::str::contains;

    fn domovoi() -> Command {
        let mut cmd = Command::cargo_bin("domovoi").unwrap();
        cmd.timeout(std::time::Duration::from_secs(60));
        cmd
    }

    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The variables the example configurations refer to.
    const EXAMPLE_ENV: [(&str, &str); 6] = [
        ("ANTHROPIC_API_KEY", "sk-ant-test"),
        ("DISCORD_BOT_TOKEN", "bot-token"),
        (
            "DISCORD_WEBHOOK_URL",
            "https://discord.com/api/webhooks/1/abc",
        ),
        ("DOMOVOI_ALERTS_TOKEN", "alerts-token"),
        ("DOMOVOI_HEARTBEAT_URL", "https://hc.example.net/ping/abc"),
        ("DOMOVOI_LOCAL_WEBHOOK_TOKEN", "local-token"),
    ];

    #[test]
    fn version_prints_target_and_commit() {
        domovoi()
            .arg("version")
            .assert()
            .success()
            .stdout(contains(env!("CARGO_PKG_VERSION")))
            .stdout(contains("commit:"))
            .stdout(contains("built:"))
            .stdout(contains("target:"))
            .stdout(contains(std::env::consts::ARCH));
    }

    #[test]
    fn help_lists_every_command() {
        let assert = domovoi().arg("--help").assert().success();
        let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
        for command in [
            "hub", "agent", "check", "notify", "selftest", "digest", "version",
        ] {
            assert!(out.contains(command), "{command} missing from:\n{out}");
        }
    }

    #[test]
    fn check_config_accepts_examples_and_reports_paths_for_errors() {
        for (file, kind) in [
            ("hub.toml", "hub"),
            ("hub.full.toml", "hub"),
            ("agent.toml", "agent"),
        ] {
            domovoi()
                .args(["check", "config", "--kind", kind])
                .arg(workspace().join("examples").join(file))
                .envs(EXAMPLE_ENV)
                .assert()
                .success()
                .stdout(contains("ok"));
        }
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("hub.toml");
        fs::write(&bad, "[hub]\nname = \"box\"\nnmae = \"typo\"\n").unwrap();
        domovoi()
            .args(["check", "config"])
            .arg(&bad)
            .assert()
            .failure()
            .stdout(contains("error: hub.nmae"))
            .stdout(contains("1 error(s)"));
        let secret = dir.path().join("secret.toml");
        fs::write(
            &secret,
            "[hub]\nname = \"box\"\n[llm.primary]\nkind = \"anthropic\"\nmodel = \"m\"\napi_key = { env = \"DOMOVOI_TEST_UNSET_KEY\" }\n",
        )
        .unwrap();
        domovoi()
            .args(["check", "config", "--kind", "hub"])
            .arg(&secret)
            .env_remove("DOMOVOI_TEST_UNSET_KEY")
            .assert()
            .failure()
            .stdout(contains("llm.primary.api_key"));
    }

    #[cfg(unix)]
    #[test]
    fn check_skills_lists_valid_and_invalid() {
        use std::os::unix::fs::PermissionsExt;

        let fixtures = workspace().join("crates/domovoi-runner/tests/fixtures/skills");
        let dir = tempfile::tempdir().unwrap();
        for skill in ["echo_args", "broken"] {
            let to = dir.path().join(skill);
            fs::create_dir(&to).unwrap();
            for file in ["skill.toml", "run"] {
                fs::copy(fixtures.join(skill).join(file), to.join(file)).unwrap();
            }
            fs::set_permissions(to.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        domovoi()
            .args(["check", "skills"])
            .arg(dir.path())
            .assert()
            .failure()
            .stdout(contains("ok     echo_args (read)"))
            .stdout(contains("error"))
            .stdout(contains("broken"))
            .stdout(contains("1 valid, 1 with issues"));
    }

    fn copy_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    #[test]
    fn check_run_executes_checks_against_a_fixture_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        copy_tree(
            &workspace().join("crates/domovoi-checks/tests/fixtures/system/healthy"),
            &root,
        );
        let config = dir.path().join("agent.toml");
        fs::write(
            &config,
            format!(
                "[agent]\nhost_name = \"box\"\nhub_url = \"wss://hub.example:7421\"\nstate_dir = '{}'\nskills_dir = '{}'\nhost_root = '{}'\n[docker]\nenabled = false\n",
                dir.path().join("state").display(),
                dir.path().join("skills").display(),
                root.display()
            ),
        )
        .unwrap();
        let assert = domovoi()
            .args(["check", "run", "--json", "--config"])
            .arg(&config)
            .assert()
            .success();
        let runs: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        let runs = runs.as_array().unwrap();
        let memory = runs
            .iter()
            .find(|r| r["check"] == "memory.available")
            .expect("memory.available ran");
        assert!(memory["error"].is_null(), "{memory}");
        assert_eq!(memory["observations"][0]["firing"], false, "{memory}");
        assert!(
            !dir.path().join("state").join("checks.json").exists(),
            "a dry run leaves no check state behind"
        );

        domovoi()
            .args(["check", "run", "--only", "memory.available", "--config"])
            .arg(&config)
            .assert()
            .success()
            .stdout(contains("memory.available:"))
            .stdout(contains("1 checks"));
        domovoi()
            .args(["check", "run", "--only", "no.such", "--config"])
            .arg(&config)
            .assert()
            .failure()
            .stderr(contains("no enabled check named no.such"));
    }

    #[test]
    fn selftest_offline_passes_all_stages() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("hub.toml");
        fs::write(
            &config,
            format!(
                "[hub]\nname = \"selftest\"\ndata_dir = '{}'\n[web]\nbind = \"127.0.0.1:0\"\n[agent_server]\nbind = \"127.0.0.1:0\"\n",
                dir.path().join("data").display()
            ),
        )
        .unwrap();
        domovoi()
            .args(["selftest", "--offline", "--config"])
            .arg(&config)
            .assert()
            .success()
            .stdout(contains("PASS  config"))
            .stdout(contains("PASS  store"))
            .stdout(contains("PASS  manager"))
            .stdout(contains("PASS  triage"))
            .stdout(contains("PASS  card"))
            .stdout(contains("PASS  gateway"))
            .stdout(contains("selftest passed"));
        assert!(
            !dir.path().join("data").exists(),
            "the selftest never touches the real data directory"
        );
    }
}
