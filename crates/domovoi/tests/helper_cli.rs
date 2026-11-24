//! The helper's and the isolation self-test's command lines, run as real processes.
#![cfg(unix)]

#[cfg(test)]
mod tests {

    use assert_cmd::Command;
    use predicates::str::contains;

    fn domovoi() -> Command {
        let mut cmd = Command::cargo_bin("domovoi").unwrap();
        cmd.timeout(std::time::Duration::from_secs(60));
        cmd
    }

    /// Whether the test process is root; those tests stand down there because root passes the check
    /// they exercise.
    fn running_as_root() -> bool {
        std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix("Uid:").map(str::to_owned))
            })
            .is_some_and(|ids| ids.split_whitespace().nth(1) == Some("0"))
    }

    #[test]
    fn helper_serve_refuses_when_stdin_is_not_a_socket() {
        domovoi()
            .args(["helper", "serve"])
            .env_remove("LISTEN_FDS")
            .write_stdin("")
            .assert()
            .failure();
    }

    #[test]
    fn helper_serve_refuses_a_nonzero_listen_fds() {
        domovoi()
            .args(["helper", "serve"])
            .env("LISTEN_FDS", "1")
            .write_stdin("")
            .assert()
            .failure()
            .stderr(contains("LISTEN_FDS"));
    }

    #[test]
    fn helper_init_refuses_when_not_root() {
        if running_as_root() {
            return;
        }
        domovoi()
            .args(["helper", "init"])
            .assert()
            .failure()
            .stderr(contains("run it as root"));
    }

    #[test]
    fn helper_repair_refuses_when_not_root_and_without_terminal() {
        if running_as_root() {
            return;
        }
        domovoi()
            .args(["helper", "repair"])
            .write_stdin("repair\n")
            .assert()
            .failure();
    }

    #[test]
    fn agent_selftest_prints_every_check_as_json_and_changes_nothing() {
        let output = domovoi()
            .args(["agent", "selftest", "--json"])
            .env("DOMOVOI_AGENT_CONFIG", "/nonexistent/agent.toml")
            .output()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["results"].as_array().unwrap().len(), 20);
    }

    #[test]
    fn agent_selftest_prints_one_line_per_check_and_exits_nonzero_on_a_fail() {
        let output = domovoi()
            .args(["agent", "selftest"])
            .env("DOMOVOI_AGENT_CONFIG", "/nonexistent/agent.toml")
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        for n in 1..=20 {
            let id = format!("ST-{n:02} ");
            assert_eq!(
                text.lines().filter(|l| l.starts_with(&id)).count(),
                1,
                "{id}: {text}"
            );
        }
        // On a host that is not set up as the three identities, at least ST-01 fails.
        if !text.lines().any(|l| l.starts_with("ST-01 pass")) {
            assert_eq!(output.status.code(), Some(1), "{text}");
        }
    }

    #[test]
    fn installer_selftest_needs_root() {
        if running_as_root() {
            return;
        }
        domovoi()
            .args(["agent", "selftest", "--installer"])
            .assert()
            .failure()
            .stderr(contains("must run as root"));
    }

    #[test]
    fn top_level_selftest_is_unchanged() {
        domovoi()
            .args(["selftest", "--help"])
            .assert()
            .success()
            .stdout(contains("--offline"));
    }
}
