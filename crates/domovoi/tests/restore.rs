//! `domovoi restore`: what it does with and without confirmation, and what it refuses.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use assert_cmd::Command;
    use domovoi_core::ports::SettingsRepo;
    use domovoi_hub::runtime::HubLock;
    use domovoi_store::{Store, StoreOptions};
    use domovoi_testkit::build::T0;

    fn domovoi() -> Command {
        let mut cmd = Command::cargo_bin("domovoi").unwrap();
        cmd.timeout(std::time::Duration::from_secs(60));
        cmd
    }

    /// A data directory with `hub.toml`, and the hub's database with `ui.theme` set.
    async fn data_dir(dir: &Path, theme: &str) -> PathBuf {
        let data = dir.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(
            dir.join("hub.toml"),
            format!("[hub]\nname = \"box\"\ndata_dir = '{}'\n", data.display()),
        )
        .unwrap();
        let db = data.join("hub.db");
        let store = Store::open(&db, StoreOptions::beside(&db, T0))
            .await
            .unwrap();
        store
            .set("ui.theme", &serde_json::json!(theme))
            .await
            .unwrap();
        store.close().await;
        data
    }

    /// A standalone backup file holding `ui.theme`.
    async fn backup(dir: &Path, theme: &str) -> PathBuf {
        let path = dir.join("backup.db");
        let store = Store::open(&path, StoreOptions::beside(&path, T0))
            .await
            .unwrap();
        store
            .set("ui.theme", &serde_json::json!(theme))
            .await
            .unwrap();
        store.close().await;
        path
    }

    async fn theme_of(path: &Path) -> Option<serde_json::Value> {
        let store = Store::open(path, StoreOptions::beside(path, T0))
            .await
            .unwrap();
        let theme = store.get("ui.theme").await.unwrap();
        store.close().await;
        theme
    }

    #[tokio::test]
    async fn restore_without_yes_changes_nothing_and_exits_2() {
        let dir = tempfile::tempdir().unwrap();
        let data = data_dir(dir.path(), "live-theme").await;
        let file = backup(dir.path(), "backup-theme").await;
        let before = fs::read(data.join("hub.db")).unwrap();

        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", dir.path().join("hub.toml").to_str().unwrap()])
            .assert()
            .code(2)
            .stdout(predicates::str::contains(
                "Nothing changed. Run again with --yes to restore.",
            ))
            .stdout(predicates::str::contains("Would restore"))
            .stdout(predicates::str::contains("Restore secret.key and tls/"));

        assert_eq!(
            fs::read(data.join("hub.db")).unwrap(),
            before,
            "nothing moved"
        );
        assert_eq!(
            theme_of(&data.join("hub.db")).await,
            Some(serde_json::json!("live-theme"))
        );
    }

    #[tokio::test]
    async fn restore_with_yes_replaces_the_database_and_keeps_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let data = data_dir(dir.path(), "live-theme").await;
        let file = backup(dir.path(), "backup-theme").await;

        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", dir.path().join("hub.toml").to_str().unwrap()])
            .arg("--yes")
            .assert()
            .success()
            .stdout(predicates::str::contains("Restored the backup"))
            .stdout(predicates::str::contains("before-restore"))
            .stdout(predicates::str::contains(
                "Data deleted since this backup was taken is back",
            ))
            .stdout(predicates::str::contains("Start the hub again."));

        assert_eq!(
            theme_of(&data.join("hub.db")).await,
            Some(serde_json::json!("backup-theme"))
        );
        let previous: Vec<PathBuf> = fs::read_dir(&data)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".before-restore-")
            })
            .collect();
        assert_eq!(previous.len(), 1, "{previous:?}");
        assert_eq!(
            theme_of(&previous[0]).await,
            Some(serde_json::json!("live-theme")),
            "the old database is kept beside the restored one"
        );
    }

    /// A backup whose audit chain is broken: three rows, the second edited.
    async fn tampered_backup(dir: &Path) -> PathBuf {
        use domovoi_core::ports::{AuditEvent, AuditLog};
        let path = dir.join("tampered.db");
        let store = Store::open(&path, StoreOptions::beside(&path, T0))
            .await
            .unwrap();
        for kind in ["test.1", "test.2", "test.3"] {
            let event = AuditEvent {
                at: T0,
                actor: "system:test".to_owned(),
                kind: kind.to_owned(),
                subject: "subject".to_owned(),
                detail: serde_json::Value::Null,
            };
            store.append(&event).await.unwrap();
        }
        store.edit_audit_row_for_tests(2).await.unwrap();
        store.close().await;
        path
    }

    #[tokio::test]
    async fn restore_refuses_a_backup_with_a_broken_audit_chain_unless_told_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let data = data_dir(dir.path(), "live-theme").await;
        let file = tampered_backup(dir.path()).await;
        let before = fs::read(data.join("hub.db")).unwrap();
        let config = dir.path().join("hub.toml");

        // The description runs the same check and says so.
        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", config.to_str().unwrap()])
            .assert()
            .failure()
            .stderr(predicates::str::contains("audit chain is broken at seq 2"))
            .stderr(predicates::str::contains("--allow-broken-audit"));
        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", config.to_str().unwrap()])
            .arg("--yes")
            .assert()
            .failure()
            .stderr(predicates::str::contains("audit chain is broken at seq 2"));
        assert_eq!(
            fs::read(data.join("hub.db")).unwrap(),
            before,
            "the live database is untouched"
        );

        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", config.to_str().unwrap()])
            .args(["--yes", "--allow-broken-audit"])
            .assert()
            .success()
            .stdout(predicates::str::contains(
                "BROKEN at seq 2: a row was edited",
            ));
    }

    #[tokio::test]
    async fn restore_says_the_audit_chain_is_intact_in_the_description() {
        let dir = tempfile::tempdir().unwrap();
        data_dir(dir.path(), "live-theme").await;
        let file = backup(dir.path(), "backup-theme").await;
        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", dir.path().join("hub.toml").to_str().unwrap()])
            .assert()
            .code(2)
            .stdout(predicates::str::contains("audit chain: "));
    }

    #[tokio::test]
    async fn restore_refuses_while_a_hub_holds_the_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let data = data_dir(dir.path(), "live-theme").await;
        let file = backup(dir.path(), "backup-theme").await;
        let before = fs::read(data.join("hub.db")).unwrap();
        let lock = HubLock::acquire(&data).unwrap();

        domovoi()
            .args(["restore"])
            .arg(&file)
            .args(["--config", dir.path().join("hub.toml").to_str().unwrap()])
            .arg("--yes")
            .assert()
            .failure()
            .stderr(predicates::str::contains("stop it before restoring"));

        assert_eq!(
            fs::read(data.join("hub.db")).unwrap(),
            before,
            "nothing moved"
        );
        drop(lock);
    }

    #[tokio::test]
    async fn restore_refuses_a_file_that_is_not_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        data_dir(dir.path(), "live-theme").await;
        let junk = dir.path().join("junk.db");
        fs::write(&junk, b"not a database").unwrap();

        domovoi()
            .args(["restore"])
            .arg(&junk)
            .args(["--config", dir.path().join("hub.toml").to_str().unwrap()])
            .arg("--yes")
            .assert()
            .failure();
    }

    #[test]
    fn restore_help_names_its_arguments_and_exit_codes() {
        let assert = domovoi().args(["restore", "--help"]).assert().success();
        let out = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
        assert!(out.contains("<BACKUP>"), "{out}");
        assert!(out.contains("--yes"), "{out}");
        assert!(out.contains("--config"), "{out}");
    }
}
