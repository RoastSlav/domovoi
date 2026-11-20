//! The transfer receiver: ordering, limits, answers and expiry.

use std::sync::Arc;

use domovoi_core::ports::Clock as _;
use domovoi_proto::messages::{
    TransferBegin, TransferChunk, TransferEnd, TransferFailureCode, TransferKind, TransferOutcome,
    TransferResult,
};
use domovoi_proto::{Base64Bytes, Message, Open, Sha256Bytes};
use domovoi_testkit::FakeClock;
use jiff::SignedDuration;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::support::start;
use crate::link::LinkHandle;
use crate::link::transfers::{IncomingTransfer, TransferFrame};
use crate::packages::TransferReceiver;
use crate::packages::staging::Staging;

const CAP: u64 = 10_000;

struct Rig {
    _dir: TempDir,
    clock: Arc<FakeClock>,
    store: Arc<Staging>,
    receiver: TransferReceiver,
}

fn rig_with(allow: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new(start()));
    let store = Staging::open(dir.path(), clock.clone()).unwrap();
    let receiver = TransferReceiver::new(store.clone(), allow, CAP, clock.clone());
    Rig {
        _dir: dir,
        clock,
        store,
        receiver,
    }
}

fn rig() -> Rig {
    rig_with(true)
}

fn kind(known: TransferKind) -> Open<TransferKind> {
    let raw = match known {
        TransferKind::PackageArtifact => "package_artifact",
        TransferKind::PackageManifest => "package_manifest",
    };
    Open::new(raw, Some(known))
}

fn artifact() -> Open<TransferKind> {
    kind(TransferKind::PackageArtifact)
}

fn begin(id: Uuid, kind: Open<TransferKind>, bytes: &[u8]) -> TransferBegin {
    TransferBegin {
        transfer_id: id,
        kind,
        size: bytes.len() as u64,
        sha256: Sha256Bytes(Sha256::digest(bytes).into()),
    }
}

fn chunk(id: Uuid, seq: u64, bytes: &[u8]) -> TransferChunk {
    TransferChunk {
        transfer_id: id,
        seq,
        bytes: Base64Bytes(bytes.to_vec()),
    }
}

fn end(id: Uuid) -> TransferEnd {
    TransferEnd { transfer_id: id }
}

/// Sends `bytes` as one transfer in 4-byte chunks and returns the answer.
fn send(rig: &mut Rig, id: Uuid, kind: Open<TransferKind>, bytes: &[u8]) -> TransferResult {
    rig.receiver.begin(&begin(id, kind, bytes));
    for (seq, part) in bytes.chunks(4).enumerate() {
        rig.receiver.chunk(&chunk(id, seq as u64, part));
    }
    rig.receiver.end(&end(id))
}

fn failed_code(result: &TransferResult) -> TransferFailureCode {
    match &result.outcome {
        TransferOutcome::Failed { code, .. } => *code.known().unwrap(),
        TransferOutcome::Stored => panic!("expected a failure"),
    }
}

#[test]
fn transfer_stores_bytes_and_answers_once_at_end() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    let result = send(&mut rig, id, artifact(), b"hello package");
    assert_eq!(result.outcome, TransferOutcome::Stored);
    assert_eq!(result.transfer_id, id);
    assert!(!rig.store.part_path(id).exists());
    let path = rig.store.claim(id, TransferKind::PackageArtifact).unwrap();
    assert_eq!(std::fs::read(path.path).unwrap(), b"hello package");
    let again = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&again), TransferFailureCode::Aborted);
}

#[tokio::test]
async fn transfer_failure_is_reported_at_end_not_before() {
    let rig = rig_with(false);
    let (handle, mut outbox) = LinkHandle::channel(8);
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(rig.receiver.serve(rx, handle, shutdown.clone()));
    let id = Uuid::now_v7();
    let frames = [
        TransferFrame::Begin(begin(id, artifact(), b"abcd")),
        TransferFrame::Chunk(chunk(id, 0, b"abcd")),
    ];
    for frame in frames {
        let request = domovoi_core::RequestId::new();
        tx.send(IncomingTransfer { id: request, frame })
            .await
            .unwrap();
    }
    tokio::task::yield_now().await;
    assert!(outbox.try_recv().is_err());
    let end_id = domovoi_core::RequestId::new();
    tx.send(IncomingTransfer {
        id: end_id,
        frame: TransferFrame::End(end(id)),
    })
    .await
    .unwrap();
    let out = outbox.recv().await.unwrap();
    assert_eq!(out.reply_to, Some(end_id));
    let Message::TransferResult(result) = out.message else {
        panic!("expected a transfer result");
    };
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
    assert!(outbox.try_recv().is_err());
    shutdown.cancel();
    task.await.unwrap();
}

#[test]
fn transfer_rejects_checksum_mismatch() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    let mut announce = begin(id, artifact(), b"abcdefgh");
    announce.sha256 = Sha256Bytes([7; 32]);
    rig.receiver.begin(&announce);
    rig.receiver.chunk(&chunk(id, 0, b"abcdefgh"));
    let result = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&result), TransferFailureCode::ChecksumMismatch);
    assert!(!rig.store.part_path(id).exists());
    assert!(!rig.store.is_staged(id));
}

#[test]
fn short_transfer_is_a_checksum_mismatch() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcdefgh"));
    rig.receiver.chunk(&chunk(id, 0, b"abcd"));
    let result = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&result), TransferFailureCode::ChecksumMismatch);
}

#[test]
fn transfer_rejects_oversize_announcement() {
    let mut rig = rig();
    let sizes = [
        (TransferKind::PackageArtifact, CAP + 1),
        (TransferKind::PackageManifest, 16 * 1024 + 1),
        (TransferKind::PackageArtifact, 0),
    ];
    for (known, size) in sizes {
        let id = Uuid::now_v7();
        let mut announce = begin(id, kind(known), b"x");
        announce.size = size;
        rig.receiver.begin(&announce);
        let result = rig.receiver.end(&end(id));
        assert_eq!(failed_code(&result), TransferFailureCode::TooLarge);
    }
}

#[test]
fn transfer_rejects_chunk_beyond_size() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcd"));
    rig.receiver.chunk(&chunk(id, 0, b"abcdefgh"));
    assert!(!rig.store.part_path(id).exists());
    let result = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&result), TransferFailureCode::TooLarge);
}

#[test]
fn transfer_rejects_out_of_order_chunk() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcdefgh"));
    rig.receiver.chunk(&chunk(id, 1, b"abcd"));
    assert!(!rig.store.part_path(id).exists());
    let result = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
}

#[test]
fn transfer_rejects_duplicate_id() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    let first = send(&mut rig, id, artifact(), b"first");
    assert_eq!(first.outcome, TransferOutcome::Stored);
    rig.receiver.begin(&begin(id, artifact(), b"second"));
    let result = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
    let path = rig.store.claim(id, TransferKind::PackageArtifact).unwrap();
    assert_eq!(std::fs::read(path.path).unwrap(), b"first");
}

#[test]
fn duplicate_begin_for_an_active_transfer_leaves_it_untouched() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcdefgh"));
    rig.receiver.chunk(&chunk(id, 0, b"abcd"));
    rig.receiver.begin(&begin(id, artifact(), b"zzzz"));
    rig.receiver.chunk(&chunk(id, 1, b"efgh"));
    assert_eq!(rig.receiver.end(&end(id)).outcome, TransferOutcome::Stored);
}

#[test]
fn transfer_rejects_unknown_kind() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    let result = send(&mut rig, id, Open::new("tarball", None), b"abcd");
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
    assert!(!rig.store.part_path(id).exists());
}

#[test]
fn end_for_unknown_transfer_is_answered_aborted() {
    let mut rig = rig();
    let result = rig.receiver.end(&end(Uuid::now_v7()));
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
}

#[test]
fn idle_transfer_is_dropped_by_the_clock() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcdefgh"));
    rig.receiver.chunk(&chunk(id, 0, b"abcd"));
    assert!(rig.store.part_path(id).exists());
    rig.clock.advance(SignedDuration::from_secs(59));
    rig.receiver.expire(rig.clock.now());
    assert!(rig.store.part_path(id).exists());
    rig.clock.advance(SignedDuration::from_secs(2));
    rig.receiver.expire(rig.clock.now());
    assert!(!rig.store.part_path(id).exists());
    let result = rig.receiver.end(&end(id));
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
}

#[test]
fn slow_but_steady_transfer_is_dropped_after_ten_minutes() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcdefghijkl"));
    rig.clock.advance(SignedDuration::from_secs(30));
    rig.receiver.chunk(&chunk(id, 0, b"abcd"));
    for _ in 0..40 {
        rig.clock.advance(SignedDuration::from_secs(15));
        rig.receiver.expire(rig.clock.now());
    }
    assert!(!rig.store.part_path(id).exists());
}

#[test]
fn staged_transfer_expires() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    send(&mut rig, id, artifact(), b"abcd");
    rig.clock.advance(SignedDuration::from_mins(14));
    rig.receiver.expire(rig.clock.now());
    assert!(rig.store.is_staged(id));
    rig.clock.advance(SignedDuration::from_mins(2));
    let late = rig.store.claim(id, TransferKind::PackageArtifact);
    assert_eq!(late.unwrap_err().code().as_str(), "transfer_expired");
    assert!(!rig.store.is_staged(id));
    assert!(!rig.store.bin_path(id).exists());
}

#[test]
fn expiry_sweep_deletes_unused_transfers() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    send(&mut rig, id, artifact(), b"abcd");
    rig.clock.advance(SignedDuration::from_mins(16));
    rig.receiver.expire(rig.clock.now());
    assert!(!rig.store.is_staged(id));
    assert!(!rig.store.bin_path(id).exists());
}

#[test]
fn claim_checks_kind_and_unknown_ids() {
    let mut rig = rig();
    let id = Uuid::now_v7();
    send(&mut rig, id, artifact(), b"abcd");
    let wrong = rig
        .store
        .claim(id, TransferKind::PackageManifest)
        .unwrap_err();
    assert_eq!(wrong.code().as_str(), "transfer_kind_mismatch");
    let unknown = rig
        .store
        .claim(Uuid::now_v7(), TransferKind::PackageArtifact)
        .unwrap_err();
    assert_eq!(unknown.code().as_str(), "transfer_unknown");
    rig.store.claim(id, TransferKind::PackageArtifact).unwrap();
}

#[test]
fn too_many_transfers_are_refused() {
    let mut rig = rig();
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
    for id in &ids {
        rig.receiver.begin(&begin(*id, artifact(), b"abcdefgh"));
    }
    let third = rig.receiver.end(&end(ids[2]));
    assert_eq!(failed_code(&third), TransferFailureCode::Storage);
    for id in &ids[..2] {
        rig.receiver.chunk(&chunk(*id, 0, b"abcd"));
        rig.receiver.chunk(&chunk(*id, 1, b"efgh"));
        assert_eq!(rig.receiver.end(&end(*id)).outcome, TransferOutcome::Stored);
    }
    for _ in 2..8 {
        let stored = send(&mut rig, Uuid::now_v7(), artifact(), b"abcd");
        assert_eq!(stored.outcome, TransferOutcome::Stored);
    }
    assert_eq!(rig.store.staged_count(), 8);
    let ninth = send(&mut rig, Uuid::now_v7(), artifact(), b"abcd");
    assert_eq!(failed_code(&ninth), TransferFailureCode::Storage);
}

#[test]
fn staged_byte_budget_is_enforced() {
    let mut rig = rig();
    let big = vec![1_u8; usize::try_from(CAP).unwrap()];
    for _ in 0..3 {
        let result = send(&mut rig, Uuid::now_v7(), artifact(), &big);
        assert_eq!(result.outcome, TransferOutcome::Stored);
    }
    let result = send(&mut rig, Uuid::now_v7(), artifact(), &big);
    assert_eq!(failed_code(&result), TransferFailureCode::Storage);
}

#[test]
fn transfers_refused_when_install_not_allowed() {
    let mut rig = rig_with(false);
    let id = Uuid::now_v7();
    let result = send(&mut rig, id, artifact(), b"abcd");
    assert_eq!(failed_code(&result), TransferFailureCode::Aborted);
    assert!(!rig.store.part_path(id).exists());
}

#[test]
fn reset_drops_arriving_transfers_but_keeps_stored_ones() {
    let mut rig = rig();
    let stored = Uuid::now_v7();
    send(&mut rig, stored, artifact(), b"abcd");
    let arriving = Uuid::now_v7();
    rig.receiver
        .begin(&begin(arriving, artifact(), b"abcdefgh"));
    rig.receiver.chunk(&chunk(arriving, 0, b"abcd"));
    rig.receiver.reset();
    assert!(!rig.store.part_path(arriving).exists());
    assert!(rig.store.is_staged(stored));
}

#[cfg(unix)]
#[test]
fn part_files_are_private() {
    use std::os::unix::fs::PermissionsExt as _;
    let mut rig = rig();
    let id = Uuid::now_v7();
    rig.receiver.begin(&begin(id, artifact(), b"abcdefgh"));
    let part = std::fs::metadata(rig.store.part_path(id)).unwrap();
    assert_eq!(part.permissions().mode() & 0o777, 0o600);
    rig.receiver.chunk(&chunk(id, 0, b"abcdefgh"));
    rig.receiver.end(&end(id));
    let stored = std::fs::metadata(rig.store.bin_path(id)).unwrap();
    assert_eq!(stored.permissions().mode() & 0o777, 0o600);
}
