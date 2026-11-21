//! `domovoi package import` and `domovoi package list`, run as real processes.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use assert_cmd::Command;
    use domovoi_hub::runtime::HubLock;
    use domovoi_testkit::{BuiltPackage, PackageFixture};
    use predicates::str::contains;

    fn domovoi() -> Command {
        let mut cmd = Command::cargo_bin("domovoi").unwrap();
        cmd.timeout(std::time::Duration::from_secs(60));
        cmd
    }

    /// A `hub.toml` that trusts the key that signed `trusted`, and its data directory.
    fn hub(dir: &Path, enabled: bool, trusted: &BuiltPackage) -> (PathBuf, PathBuf) {
        let data = dir.join("data");
        fs::create_dir_all(&data).unwrap();
        let config = dir.join("hub.toml");
        fs::write(
            &config,
            format!(
                "[hub]\nname = \"box\"\ndata_dir = '{}'\n\n[packages]\nenabled = {enabled}\n\n\
                 [[packages.owner_keys]]\nid = \"{}\"\npublic_key = \"{}\"\nlabel = \"Owner\"\n",
                data.display(),
                trusted.key_id,
                trusted.public_key_b64
            ),
        )
        .unwrap();
        (config, data)
    }

    fn write_package(dir: &Path, built: &BuiltPackage) -> (PathBuf, PathBuf) {
        let archive = dir.join("tools-1.0.0.tar.zst");
        let signature = dir.join("tools-1.0.0.sig.toml");
        fs::write(&archive, &built.archive).unwrap();
        fs::write(&signature, &built.sig_toml).unwrap();
        (archive, signature)
    }

    fn built() -> BuiltPackage {
        PackageFixture::new("tools", "1.0.0")
            .unwrap()
            .skill("probe_it")
            .build()
    }

    #[test]
    fn package_import_stores_and_lists() {
        let dir = tempfile::tempdir().unwrap();
        let package = built();
        let (config, data) = hub(dir.path(), true, &package);
        let (archive, signature) = write_package(dir.path(), &package);
        let config = config.to_str().unwrap();

        domovoi()
            .args(["package", "import"])
            .arg(&archive)
            .arg(&signature)
            .args(["--config", config])
            .assert()
            .success()
            .stdout(contains("Imported"))
            .stdout(contains("tools"))
            .stdout(contains("1.0.0"))
            .stdout(contains(package.archive_sha256.to_string()))
            .stdout(contains(package.key_id.to_string()))
            .stdout(contains("Owner"));

        let digest = package.archive_sha256.to_string();
        let stored = data
            .join("packages")
            .join(&digest[..2])
            .join(format!("{digest}.tar.zst"));
        assert_eq!(fs::read(stored).unwrap(), package.archive);

        domovoi()
            .args(["package", "list", "--config", config])
            .assert()
            .success()
            .stdout(contains("tools"))
            .stdout(contains(digest.as_str()))
            .stdout(contains("Owner"));

        domovoi()
            .args(["package", "import"])
            .arg(&archive)
            .arg(&signature)
            .args(["--config", config])
            .assert()
            .success()
            .stdout(contains("Already imported"));
    }

    #[test]
    fn package_import_reports_unknown_key_with_nonzero_exit() {
        let dir = tempfile::tempdir().unwrap();
        let (config, data) = hub(dir.path(), true, &built());
        let stranger = built();
        let (archive, signature) = write_package(dir.path(), &stranger);

        domovoi()
            .args(["package", "import"])
            .arg(&archive)
            .arg(&signature)
            .args(["--config", config.to_str().unwrap()])
            .assert()
            .failure()
            .stderr(contains("unknown_key"))
            .stderr(contains(stranger.key_id.to_string()));

        let digest = stranger.archive_sha256.to_string();
        assert!(
            !data
                .join("packages")
                .join(&digest[..2])
                .join(format!("{digest}.tar.zst"))
                .exists(),
            "a refused package is not stored"
        );
    }

    #[test]
    fn package_import_refuses_when_packages_are_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let package = built();
        let (config, _) = hub(dir.path(), false, &package);
        let (archive, signature) = write_package(dir.path(), &package);

        domovoi()
            .args(["package", "import"])
            .arg(&archive)
            .arg(&signature)
            .args(["--config", config.to_str().unwrap()])
            .assert()
            .failure()
            .stderr(contains("packages_disabled"));
    }

    #[test]
    fn package_import_refuses_while_a_hub_runs() {
        let dir = tempfile::tempdir().unwrap();
        let package = built();
        let (config, data) = hub(dir.path(), true, &package);
        let (archive, signature) = write_package(dir.path(), &package);
        let lock = HubLock::acquire(&data).unwrap();

        domovoi()
            .args(["package", "import"])
            .arg(&archive)
            .arg(&signature)
            .args(["--config", config.to_str().unwrap()])
            .assert()
            .failure()
            .stderr(contains("a hub is running"));
        assert!(!data.join("packages").exists(), "nothing was stored");
        drop(lock);
    }

    #[test]
    fn package_list_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (config, _) = hub(dir.path(), true, &built());

        domovoi()
            .args(["package", "list", "--config", config.to_str().unwrap()])
            .assert()
            .success()
            .stdout(contains("No packages imported."));
    }
}
