//! `domovoi check run` in container mode: journal and systemd checks are unavailable, not
//! errors, and nothing is run for them.

use std::sync::Arc;

use domovoi_agent::AgentDeps;
use domovoi_agent::dry_run::{run_checks_once, summary};
use domovoi_config::MapEnv;
use domovoi_testkit::{FakeProcessExecutor, ListRedactor};

use crate::common::NoStats;
use crate::harness::{clock, empty_fs, key};

#[tokio::test]
async fn journal_checks_are_unavailable_in_a_container() {
    let dir = tempfile::tempdir().unwrap();
    let text = format!(
        "[agent]\nhost_name = \"nas\"\nhub_url = \"wss://hub.example:7421\"\nstate_dir = '{}'\nskills_dir = '{}'\nhost_root = '/host'\n[docker]\nenabled = false\n",
        dir.path().display(),
        dir.path().join("skills").display(),
    );
    let config = domovoi_config::parse(&text, &MapEnv::new()).unwrap().0;
    let executor = Arc::new(FakeProcessExecutor::new());
    let deps = AgentDeps {
        clock: clock(),
        executor: executor.clone(),
        hostfs: empty_fs(&dir),
        toolfs: empty_fs(&dir),
        fs_stats: Arc::new(NoStats),
        container_source: None,
        redactor: Arc::new(ListRedactor::new(Vec::<String>::new())),
    };

    for check in ["kernel.errors", "security.ssh_login"] {
        let runs = run_checks_once(&config, &deps, Some(&key(check)))
            .await
            .unwrap();
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.error, None, "{check}");
        let why = run.unavailable.as_deref().unwrap_or_default();
        assert!(
            why.contains("journalctl") && why.contains("container"),
            "{why}"
        );
        assert_eq!(summary(&runs).get("unavailable"), Some(&1));
    }
    assert!(executor.calls().is_empty());
}
