//! Claiming a staged transfer: an install in progress is never swept by the expiry timer.

use std::sync::Arc;

use domovoi_core::ports::Clock as _;
use domovoi_proto::messages::TransferKind;
use domovoi_testkit::FakeClock;
use jiff::SignedDuration;
use tempfile::TempDir;
use uuid::Uuid;

use super::support::start;
use crate::packages::staging::Staging;

struct Rig {
    _dir: TempDir,
    clock: Arc<FakeClock>,
    store: Arc<Staging>,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(start()));
    let store = Staging::open(dir.path(), clock.clone()).unwrap();
    Rig {
        _dir: dir,
        clock,
        store,
    }
}

fn stage(rig: &Rig, kind: TransferKind, bytes: &[u8]) -> Uuid {
    let id = Uuid::now_v7();
    std::fs::write(rig.store.bin_path(id), bytes).unwrap();
    rig.store
        .register_staged(id, kind, u64::try_from(bytes.len()).unwrap());
    id
}

#[test]
fn claimed_transfers_survive_the_expiry_sweep_until_deleted() {
    let rig = rig();
    let artifact = stage(&rig, TransferKind::PackageArtifact, b"archive");
    let signature = stage(&rig, TransferKind::PackageManifest, b"sig");
    let unused = stage(&rig, TransferKind::PackageArtifact, b"unused");
    let claimed_artifact = rig
        .store
        .claim(artifact, TransferKind::PackageArtifact)
        .unwrap();
    let claimed_signature = rig
        .store
        .claim(signature, TransferKind::PackageManifest)
        .unwrap();

    rig.clock.advance(SignedDuration::from_mins(16));
    let swept = rig.store.expire_staged(rig.clock.now());

    assert_eq!(swept, vec![unused]);
    assert!(claimed_artifact.path.exists());
    assert!(claimed_signature.path.exists());
    assert_eq!(rig.store.staged_count(), 2);
    assert_eq!(rig.store.staged_bytes(), 10);

    rig.store.delete_claimed(&claimed_artifact);
    rig.store.delete_claimed(&claimed_signature);
    assert!(!claimed_artifact.path.exists());
    assert!(!claimed_signature.path.exists());
    assert_eq!(rig.store.staged_count(), 0);
    assert_eq!(rig.store.staged_bytes(), 0);
}

#[test]
fn a_transfer_cannot_be_claimed_twice() {
    let rig = rig();
    let id = stage(&rig, TransferKind::PackageArtifact, b"archive");
    rig.store.claim(id, TransferKind::PackageArtifact).unwrap();
    let again = rig.store.claim(id, TransferKind::PackageArtifact);
    assert_eq!(again.unwrap_err().code().as_str(), "transfer_unknown");
}
