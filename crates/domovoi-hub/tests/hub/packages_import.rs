//! Deleting an imported package: refused while an install job uses it, and the shared file
//! stays while any other row still has the same digest.

use std::sync::Arc;

use base64::Engine as _;
use domovoi_config::{PackagesHubSection, TrustedKey};
use domovoi_core::package::{KeyId, PackageName, PackageVersion, signing_message};
use domovoi_core::ports::{
    AuditFilter, AuditLog, InstallRepo, InstallWrite, Page, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{
    Actor, ExecutionId, HostId, InstallJob, InstallJobId, InstallKind, InstallTarget,
    InstallTargetId, PlanHash, StoredAuthorization, TargetGuard, TargetStatus, UserId,
};
use domovoi_hub::packages::{
    ImportContext, PackageImporter, PackageKeys, PackageStore, ReaderSource,
};
use domovoi_testkit::approvals::{human, provenance};
use domovoi_testkit::build::{T0, at};
use domovoi_testkit::{FakeClock, InMemoryStore, PackageFixture};
use ed25519_dalek::{Signer as _, SigningKey};

fn signature(archive: &[u8], version: &str, key: &SigningKey) -> String {
    let digest = domovoi_core::package::ArchiveDigest::of_bytes(archive);
    let key_id = KeyId::of_public_key(&key.verifying_key().to_bytes());
    let name = PackageName::parse("tools").unwrap();
    let version = PackageVersion::parse(version).unwrap();
    let message = signing_message(&name, &version, &digest, &key_id);
    let signed = base64::engine::general_purpose::STANDARD.encode(key.sign(&message).to_bytes());
    format!(
        "format = 1\nalgorithm = \"ed25519\"\nkey_id = \"{key_id}\"\npackage = \"tools\"\nversion = \"{version}\"\narchive_sha256 = \"{digest}\"\nsignature = \"{signed}\"\n"
    )
}

#[tokio::test]
async fn delete_refused_while_referenced_and_keeps_shared_file() {
    let key = SigningKey::from_bytes(&[3; 32]);
    let built = PackageFixture::new("tools", "1.0.0")
        .unwrap()
        .sign_with(&key)
        .build();
    let section = PackagesHubSection {
        enabled: true,
        owner_keys: vec![TrustedKey {
            id: built.key_id.to_string(),
            public_key: built.public_key_b64.clone(),
            label: None,
        }],
        ..PackagesHubSection::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(InMemoryStore::default());
    let importer = PackageImporter::new(
        section.clone(),
        Arc::new(PackageKeys::from_config(&section).unwrap()),
        PackageStore::open(dir.path()).await.unwrap(),
        db.clone(),
        db.clone(),
        Arc::new(FakeClock::new(T0)),
    );
    let owner = Actor::User(UserId::new());
    let ctx = ImportContext {
        actor: owner.clone(),
        audit_rejections: true,
    };

    // The same archive under two versions: one file, two rows.
    let mut rows = Vec::new();
    for version in ["1.0.0", "1.0.1"] {
        let sig = signature(&built.archive, version, &key);
        let mut source = ReaderSource::new(built.archive.as_slice());
        rows.push(
            importer
                .import(&ctx, &mut source, sig.as_bytes())
                .await
                .unwrap()
                .artifact,
        );
    }
    let file = importer.store().path_of(&built.archive_sha256);
    assert!(file.exists());

    // A queued install of 1.0.0 pins that row.
    let job = InstallJob {
        id: InstallJobId::new(),
        kind: InstallKind::Install,
        name: rows[0].name.clone(),
        version: rows[0].version.clone(),
        archive_sha256: Some(rows[0].archive_sha256),
        key_id: Some(rows[0].key_id.clone()),
        requested_by: UserId::new(),
        created_at: at(5),
        job_plan_hash: "ab".repeat(32).parse::<PlanHash>().unwrap(),
    };
    let target = InstallTarget {
        id: InstallTargetId::new(),
        job: job.id,
        host: HostId::new(),
        attempt: 1,
        execution: ExecutionId::new(),
        status: TargetStatus::Queued,
        plan_hash: job.job_plan_hash,
        authorization: StoredAuthorization::new(human(UserId::new()), provenance()),
        expires_at: at(900),
        artifact_transfer: None,
        signature_transfer: None,
        dispatched_session: None,
        dispatched_epoch: None,
        failure: None,
        outputs: None,
        outputs_confirmed: false,
        queued_at: at(5),
        started_at: None,
        finished_at: None,
        version: 1,
    };
    db.commit(
        Transaction::new().write(StateWrite::Install(InstallWrite::CreateJob {
            job: job.clone(),
            targets: vec![target.clone()],
        })),
    )
    .await
    .unwrap();

    let err = importer.delete(&owner, rows[0].id).await.unwrap_err();
    assert_eq!(err.code(), "package_in_use");
    assert_eq!(db.artifacts().await.unwrap().len(), 2);

    // The other row is free, but the file stays for the one still pinned.
    importer.delete(&owner, rows[1].id).await.unwrap();
    assert_eq!(db.artifacts().await.unwrap().len(), 1);
    assert!(
        file.exists(),
        "the shared file stays while another row has its digest"
    );

    // Once the job is finished, the last row and the file go.
    let mut done = target.clone();
    done.status = TargetStatus::NotRun;
    done.finished_at = Some(at(10));
    db.commit(
        Transaction::new().write(StateWrite::Install(InstallWrite::Target {
            target: done,
            expected: TargetGuard {
                status: TargetStatus::Queued,
                version: 1,
            },
        })),
    )
    .await
    .unwrap();
    importer.delete(&owner, rows[0].id).await.unwrap();
    assert!(!file.exists());
    assert_eq!(
        importer
            .delete(&owner, rows[0].id)
            .await
            .unwrap_err()
            .code(),
        "no_such_package"
    );
    let deleted = db
        .list(
            &AuditFilter {
                kind: Some("package.deleted".to_owned()),
                ..AuditFilter::default()
            },
            &Page {
                cursor: None,
                limit: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted.items.len(), 2);
}
