//! The import pipeline: what it accepts, what it refuses and what each refusal leaves behind.

use domovoi_core::package::{ArchiveDigest, KeyId};
use domovoi_testkit::PackageFixture;

use super::{
    Chunks, Rig, owner_ctx, owner_key, package_with, section, signature_for, trusted, zstd_prefixed,
};
use crate::packages::{ImportError, ReaderSource};

#[tokio::test]
async fn owner_import_requires_trusted_owner_key() {
    let rig = Rig::new().await;

    let stranger = package_with("tools", "1.0.0", &owner_key(9));
    let err = rig.import(&stranger).await.unwrap_err();
    assert_eq!(err.code(), "unknown_key");
    assert!(err.to_string().contains(&stranger.key_id.to_string()));
    assert!(
        !err.to_string()
            .contains(&KeyId::of_public_key(&rig.owner.verifying_key().to_bytes()).to_string()),
        "the message must not list the owner keys"
    );
    assert_eq!(
        rig.files(),
        Vec::<String>::new(),
        "a refusal leaves no file"
    );
    let rejected = rig.audit("package.import_rejected").await;
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].detail["code"], "unknown_key");
    assert_eq!(rejected[0].subject, "package:tools@1.0.0");

    let built = package_with("tools", "1.0.0", &rig.owner);
    let outcome = rig.import(&built).await.unwrap();
    assert!(!outcome.already_imported);
    let artifact = outcome.artifact;
    assert_eq!(artifact.signer_label.as_deref(), Some("Owner"));
    assert_eq!(artifact.key_id, built.key_id);
    assert_eq!(artifact.archive_sha256, built.archive_sha256);
    assert_eq!(artifact.archive_size, built.archive.len() as u64);
    assert_eq!(artifact.signature_toml, built.sig_toml);
    assert_eq!(artifact.imported_at, domovoi_testkit::build::T0);
    let stored = std::fs::read(rig.importer.store().path_of(&built.archive_sha256)).unwrap();
    assert_eq!(stored, built.archive);
    let imported = rig.audit("package.imported").await;
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].subject, "package:tools@1.0.0");
    assert!(!imported[0].detail.to_string().contains("signature"));
}

#[tokio::test]
async fn import_refuses_revoked_key_even_when_listed() {
    let owner = owner_key(1);
    let mut config = section(vec![trusted(&owner, "Owner")]);
    config.revoked = vec![KeyId::of_public_key(&owner.verifying_key().to_bytes()).to_string()];
    let rig = Rig::with(config, owner.clone()).await;

    let err = rig
        .import(&package_with("tools", "1.0.0", &owner))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "revoked_key");
    // A revoked key that was never listed is refused as revoked too.
    let mut config = section(Vec::new());
    let other = owner_key(4);
    config.revoked = vec![KeyId::of_public_key(&other.verifying_key().to_bytes()).to_string()];
    let rig = Rig::with(config, other.clone()).await;
    let err = rig
        .import(&package_with("tools", "1.0.0", &other))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "revoked_key");
    assert_eq!(rig.files(), Vec::<String>::new());
}

#[tokio::test]
async fn import_rejects_bad_signature_and_digest_mismatch() {
    let rig = Rig::new().await;

    let built = package_with("tools", "1.0.0", &rig.owner);
    let other = package_with("tools", "2.0.0", &rig.owner);
    let signature_of = |toml: &str| {
        toml.lines()
            .find(|line| line.starts_with("signature = "))
            .unwrap()
            .to_owned()
    };
    let swapped = built.sig_toml.replace(
        &signature_of(&built.sig_toml),
        &signature_of(&other.sig_toml),
    );
    let mut source = ReaderSource::new(built.archive.as_slice());
    let err = rig
        .importer
        .import(&owner_ctx(), &mut source, swapped.as_bytes())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "bad_signature");

    let garbled = built.sig_toml.replace(
        &signature_of(&built.sig_toml),
        "signature = \"not base64!\"",
    );
    let mut source = ReaderSource::new(built.archive.as_slice());
    let err = rig
        .importer
        .import(&owner_ctx(), &mut source, garbled.as_bytes())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "bad_signature");

    let tampered = PackageFixture::new("tools", "1.0.0")
        .unwrap()
        .sign_with(&rig.owner)
        .tamper_digest()
        .build();
    let err = rig.import(&tampered).await.unwrap_err();
    assert_eq!(err.code(), "digest_mismatch");
    assert_eq!(rig.files(), Vec::<String>::new());
    assert_eq!(rig.audit("package.import_rejected").await.len(), 3);
    assert!(rig.audit("package.imported").await.is_empty());
}

#[tokio::test]
async fn import_rejects_unknown_fields_and_oversize_signature_file() {
    let rig = Rig::new().await;
    let built = package_with("tools", "1.0.0", &rig.owner);
    let attempts = [
        format!("{}extra = 1\n", built.sig_toml),
        format!("{}# {}\n", built.sig_toml, "x".repeat(16 * 1024)),
        built.sig_toml.replace("format = 1", "format = 2"),
        built.sig_toml.replace("ed25519", "rsa"),
        "not toml at all [".to_owned(),
        String::new(),
    ];
    for text in attempts {
        let mut source = ReaderSource::new(built.archive.as_slice());
        let err = rig
            .importer
            .import(&owner_ctx(), &mut source, text.as_bytes())
            .await
            .unwrap_err();
        assert_eq!(err.code(), "signature_file_invalid", "{err}");
    }
    let mut source = ReaderSource::new(built.archive.as_slice());
    let err = rig
        .importer
        .import(&owner_ctx(), &mut source, &[0xff, 0xfe, 0x00])
        .await
        .unwrap_err();
    assert_eq!(err.code(), "signature_file_invalid");
    assert_eq!(rig.files(), Vec::<String>::new());
}

#[tokio::test]
async fn import_rejects_non_zstd_archive() {
    let rig = Rig::new().await;
    let sig = |archive: &[u8]| signature_for(archive, "tools", "1.0.0", &rig.owner);

    for archive in [
        b"PK\x03\x04 a zip file".to_vec(),
        b"\x28\xB5\x2F".to_vec(),
        Vec::new(),
    ] {
        let signature = sig(&archive);
        let mut source = Chunks::new(vec![archive]);
        let err = rig
            .importer
            .import(&owner_ctx(), &mut source, signature.as_bytes())
            .await
            .unwrap_err();
        assert_eq!(err.code(), "not_zstd");
    }
    assert_eq!(
        rig.files(),
        Vec::<String>::new(),
        "the partial file is deleted"
    );

    // The magic number may arrive split across chunks.
    let archive = zstd_prefixed(100);
    let signature = sig(&archive);
    let mut source = Chunks::new(vec![
        archive[..2].to_vec(),
        archive[2..3].to_vec(),
        archive[3..].to_vec(),
    ]);
    rig.importer
        .import(&owner_ctx(), &mut source, signature.as_bytes())
        .await
        .unwrap();
}

#[tokio::test]
async fn import_streams_and_caps_the_archive() {
    let owner = owner_key(1);
    let mut config = section(vec![trusted(&owner, "Owner")]);
    config.max_artifact_bytes = 1024;
    let rig = Rig::with(config, owner.clone()).await;

    let mut chunks: Vec<Vec<u8>> = vec![zstd_prefixed(512)];
    chunks.extend((0..100).map(|_| vec![1u8; 512]));
    let mut source = Chunks::new(chunks);
    let err = rig
        .importer
        .import(&owner_ctx(), &mut source, b"unused")
        .await
        .unwrap_err();
    assert!(matches!(err, ImportError::TooLarge { max: 1024 }));
    assert_eq!(err.code(), "too_large");
    assert_eq!(
        source.taken, 3,
        "the source is not read past the first byte over the cap"
    );
    assert_eq!(rig.files(), Vec::<String>::new());
    assert_eq!(rig.audit("package.import_rejected").await.len(), 1);

    // Exactly the cap is fine, and arrives in many small chunks.
    let archive = zstd_prefixed(1024);
    let signature = signature_for(&archive, "tools", "1.0.0", &owner);
    let mut source = ReaderSource::with_chunk_size(archive.as_slice(), 100);
    let outcome = rig
        .importer
        .import(&owner_ctx(), &mut source, signature.as_bytes())
        .await
        .unwrap();
    assert_eq!(outcome.artifact.archive_size, 1024);
    assert_eq!(
        outcome.artifact.archive_sha256,
        ArchiveDigest::of_bytes(&archive)
    );
}

#[tokio::test]
async fn import_is_idempotent_and_conflicts_on_other_digest() {
    let rig = Rig::new().await;
    let built = package_with("tools", "1.0.0", &rig.owner);
    let first = rig.import(&built).await.unwrap();
    let again = rig.import(&built).await.unwrap();
    assert!(again.already_imported);
    assert_eq!(again.artifact.id, first.artifact.id);
    assert_eq!(rig.audit("package.imported").await.len(), 1);
    assert_eq!(rig.files().len(), 1);

    let different = PackageFixture::new("tools", "1.0.0")
        .unwrap()
        .file("extra.txt", b"changed")
        .sign_with(&rig.owner)
        .build();
    let err = rig.import(&different).await.unwrap_err();
    assert_eq!(err.code(), "package_conflict");
    assert_eq!(rig.files().len(), 1, "the other archive was not kept");
    assert_eq!(rig.audit("package.import_rejected").await.len(), 1);

    // A file that went missing is restored by importing the same archive again.
    std::fs::remove_file(rig.importer.store().path_of(&built.archive_sha256)).unwrap();
    let repaired = rig.import(&built).await.unwrap();
    assert!(repaired.already_imported);
    assert!(rig.importer.store().contains(&built.archive_sha256).await);
}

#[tokio::test]
async fn import_respects_store_caps() {
    let owner = owner_key(1);
    let mut config = section(vec![trusted(&owner, "Owner")]);
    config.max_artifacts = 1;
    let rig = Rig::with(config, owner.clone()).await;
    rig.import(&package_with("one", "1.0.0", &owner))
        .await
        .unwrap();
    let err = rig
        .import(&package_with("two", "1.0.0", &owner))
        .await
        .unwrap_err();
    assert_eq!(err.code(), "store_full");
    // The package that is already there still imports as a repeat.
    let repeat = rig.import(&package_with("one", "1.0.0", &owner)).await;
    assert!(repeat.is_ok_and(|outcome| outcome.already_imported));

    let mut config = section(vec![trusted(&owner, "Owner")]);
    config.max_store_bytes = 1024 * 1024;
    let rig = Rig::with(config, owner.clone()).await;
    let big = |name: &str| {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let noise: Vec<u8> = (0..600_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        PackageFixture::new(name, "1.0.0")
            .unwrap()
            .file("noise.bin", &noise)
            .sign_with(&owner)
            .build()
    };
    rig.import(&big("first")).await.unwrap();
    let err = rig.import(&big("second")).await.unwrap_err();
    assert_eq!(err.code(), "store_full");
    assert_eq!(rig.files().len(), 1);
}

#[tokio::test]
async fn import_never_opens_the_archive() {
    let rig = Rig::new().await;
    // Valid magic, then bytes no zstd decoder would accept: the hub only counts and hashes.
    let archive = zstd_prefixed(4096);
    let signature = signature_for(&archive, "opaque", "1.0.0", &rig.owner);
    let mut source = Chunks::new(vec![archive.clone()]);
    let outcome = rig
        .importer
        .import(&owner_ctx(), &mut source, signature.as_bytes())
        .await
        .unwrap();
    let stored = std::fs::read(
        rig.importer
            .store()
            .path_of(&outcome.artifact.archive_sha256),
    )
    .unwrap();
    assert_eq!(stored, archive);

    // No decompression or archive crate is linked into the hub.
    let manifest = include_str!("../../../Cargo.toml");
    for line in manifest.lines() {
        for forbidden in ["zstd", "ruzstd", "flate2", "tar "] {
            assert!(!line.starts_with(forbidden), "hub depends on {forbidden}");
        }
    }
    for source in [
        include_str!("../import.rs"),
        include_str!("../store.rs"),
        include_str!("../verify.rs"),
    ] {
        for forbidden in ["ruzstd", "zstd::", "flate2", "tar::", "Archive::new"] {
            assert!(!source.contains(forbidden), "the hub names {forbidden}");
        }
    }
}
