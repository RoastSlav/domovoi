//! The file store: where files go, what the startup sweep removes and what permissions they get.

use std::collections::BTreeSet;

use domovoi_core::package::ArchiveDigest;

use super::{Rig, package_with};

#[tokio::test]
async fn startup_sweep_removes_orphans_and_tmp_files() {
    let rig = Rig::new().await;
    let kept = rig
        .import(&package_with("tools", "1.0.0", &rig.owner))
        .await
        .unwrap()
        .artifact;

    let root = rig.dir.path().join("packages");
    std::fs::write(root.join(".tmp").join("crashed.part"), b"half an upload").unwrap();
    let orphan = ArchiveDigest::of_bytes(b"nobody imported this");
    let orphan_path = rig.importer.store().path_of(&orphan);
    std::fs::create_dir_all(orphan_path.parent().unwrap()).unwrap();
    std::fs::write(&orphan_path, b"orphan").unwrap();
    let stranger = root.join(&orphan.to_string()[..2]).join("notes.txt");
    std::fs::write(&stranger, b"not ours").unwrap();

    let report = rig.importer.sweep().await.unwrap();
    assert_eq!(report.tmp_files.len(), 1);
    assert_eq!(report.unindexed, vec![orphan_path.clone()]);
    assert!(!orphan_path.exists());
    assert!(
        stranger.exists(),
        "files that do not look like archives are left alone"
    );
    assert!(rig.importer.store().contains(&kept.archive_sha256).await);
    assert!(
        std::fs::read_dir(root.join(".tmp"))
            .unwrap()
            .next()
            .is_none(),
        ".tmp is empty"
    );
    let usage = rig.importer.store().usage().await.unwrap();
    assert_eq!(usage.files, 1);
    assert_eq!(usage.bytes, kept.archive_size);

    let nothing = rig.importer.store().sweep(&BTreeSet::new()).await.unwrap();
    assert_eq!(
        nothing.unindexed.len(),
        1,
        "with an empty index every archive is an orphan"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn stored_files_and_directories_are_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let rig = Rig::new().await;
    let built = package_with("tools", "1.0.0", &rig.owner);
    rig.import(&built).await.unwrap();
    let file = rig.importer.store().path_of(&built.archive_sha256);
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    assert_eq!(mode(&rig.dir.path().join("packages").join(".tmp")), 0o700);
}

#[tokio::test]
async fn packages_are_refused_while_disabled() {
    let owner = super::owner_key(1);
    let mut config = super::section(vec![super::trusted(&owner, "Owner")]);
    config.enabled = false;
    let rig = Rig::with(config, owner.clone()).await;
    let err = rig
        .import(&package_with("tools", "1.0.0", &owner))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "packages_disabled");
    assert_eq!(rig.files(), Vec::<String>::new());
    assert!(rig.audit("package.import_rejected").await.is_empty());
}

#[tokio::test]
async fn stored_archive_reads_back_in_link_sized_chunks() {
    use domovoi_core::install::MAX_CHUNK_BYTES;
    use domovoi_core::ports::{FileError, PackageFiles as _};

    let rig = Rig::new().await;
    let archive = super::zstd_prefixed(MAX_CHUNK_BYTES * 2 + 5);
    let signature = super::signature_for(&archive, "tools", "1.0.0", &rig.owner);
    let mut source = super::Chunks::new(vec![archive.clone()]);
    let artifact = rig
        .importer
        .import(&super::owner_ctx(), &mut source, signature.as_bytes())
        .await
        .unwrap()
        .artifact;

    let mut reader = rig
        .importer
        .store()
        .open(&artifact.archive_sha256)
        .await
        .unwrap();
    let mut sizes = Vec::new();
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next_chunk().await.unwrap() {
        sizes.push(chunk.len());
        bytes.extend(chunk);
    }
    assert_eq!(sizes, vec![MAX_CHUNK_BYTES, MAX_CHUNK_BYTES, 5]);
    assert_eq!(bytes, archive);

    let missing = ArchiveDigest::of_bytes(b"nothing");
    assert!(matches!(
        rig.importer.store().open(&missing).await,
        Err(FileError::NotFound)
    ));
}
