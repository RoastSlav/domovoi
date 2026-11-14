//! Fault injection for `Spool::push`: every failure gives its sequence number back, puts the
//! spool file back the way it was, and never leaves a batch the caller was told had failed.

use std::path::{Path, PathBuf};

use domovoi_core::{CheckKey, HostId, Severity, Signal, SourceKind, TargetKey};
use domovoi_testkit::build::T0;

use super::{MAX_BYTES, SPOOL_FILE, Spool, Stage};

fn signal(name: &str) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        HostId::new(),
        CheckKey::parse("test.check").unwrap(),
        Some(TargetKey::parse(name).unwrap()),
        Severity::Warning,
        format!("{name} fired"),
        T0,
    )
}

fn spool_file(dir: &Path) -> PathBuf {
    dir.join(SPOOL_FILE)
}

fn contents(dir: &Path) -> Vec<u8> {
    std::fs::read(spool_file(dir)).unwrap()
}

/// The targets of every pending batch, oldest first.
fn targets(spool: &Spool) -> Vec<Vec<String>> {
    spool
        .pending_after(None)
        .iter()
        .map(|batch| {
            batch
                .signals
                .iter()
                .map(|signal| signal.target().map_or(String::new(), |t| t.to_string()))
                .collect()
        })
        .collect()
}

/// The sequence numbers of every pending batch, oldest first.
fn seqs(spool: &Spool) -> Vec<u64> {
    spool
        .pending_after(None)
        .iter()
        .map(|batch| batch.batch_seq)
        .collect()
}

fn one(target: &str) -> Vec<String> {
    vec![target.to_owned()]
}

#[test]
fn open_failure_leaves_the_file_and_memory_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    let before = contents(dir.path());
    let bytes = spool.bytes;

    spool.inject(&[Stage::Open]);
    assert!(spool.push(vec![signal("b")]).is_err());

    assert_eq!(spool.len(), 1);
    assert_eq!(targets(&spool), [one("a")]);
    assert_eq!(seqs(&spool), [0]);
    assert_eq!(spool.bytes, bytes, "the byte count is rolled back");
    assert_eq!(contents(dir.path()), before, "the file is untouched");

    let mut reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a")]);
    assert_eq!(
        reopened.push(vec![signal("c")]).unwrap(),
        1,
        "the failed batch's number is reused"
    );
    assert_eq!(seqs(&reopened), [0, 1]);
}

#[test]
fn a_partial_append_is_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    let before = contents(dir.path());

    spool.inject(&[Stage::Append]);
    assert!(spool.push(vec![signal("b")]).is_err());

    assert_eq!(targets(&spool), [one("a")], "the live spool is unchanged");
    assert_eq!(seqs(&spool), [0]);
    assert_eq!(
        contents(dir.path()),
        before,
        "the half-written line is gone"
    );
    assert!(
        !String::from_utf8_lossy(&contents(dir.path())).contains("b fired"),
        "no part of the failed batch is left in the file"
    );

    let mut reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a")]);
    assert_eq!(reopened.len(), 1, "no malformed batch appears");
    assert_eq!(reopened.push(vec![signal("c")]).unwrap(), 1);
    let reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a"), one("c")]);
}

#[test]
fn a_failed_flush_is_undone() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    let before = contents(dir.path());

    // The whole line reached the file; only the flush failed.
    spool.inject(&[Stage::Sync]);
    assert!(spool.push(vec![signal("b")]).is_err());

    assert_eq!(targets(&spool), [one("a")]);
    assert_eq!(seqs(&spool), [0]);
    assert_eq!(contents(dir.path()), before);
    let mut reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a")]);
    assert_eq!(reopened.push(vec![signal("c")]).unwrap(), 1);
}

#[test]
fn a_failed_metadata_save_after_an_append_is_undone() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    let before = contents(dir.path());

    // The line was written, then `spool.json` failed: the line must not survive on its own.
    spool.inject(&[Stage::SaveMeta]);
    assert!(spool.push(vec![signal("b")]).is_err());

    assert_eq!(targets(&spool), [one("a")]);
    assert_eq!(seqs(&spool), [0]);
    assert_eq!(contents(dir.path()), before, "the appended line is undone");
    let mut reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a")]);
    assert_eq!(seqs(&reopened), [0]);
    assert_eq!(
        reopened.push(vec![signal("c")]).unwrap(),
        1,
        "the number is reused, and no gap appears"
    );
    let reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a"), one("c")]);
}

#[test]
fn a_failed_metadata_save_after_a_rewrite_restores_the_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    // One batch fits: every further push makes `shrink` drop the oldest and rewrite the file.
    let mut spool = Spool::open(dir.path()).unwrap().with_limits(1, MAX_BYTES);
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    assert_eq!(spool.push(vec![signal("b")]).unwrap(), 1);
    assert_eq!(targets(&spool), [one("b")], "a was dropped when b arrived");
    let before = contents(dir.path());

    spool.inject(&[Stage::SaveMeta]);
    assert!(spool.push(vec![signal("c")]).is_err());

    // `shrink` dropped b on the way in and does not put it back: that is the documented
    // handling of batches shrink removed, not part of the failed batch's rollback.
    assert!(spool.pending_after(None).is_empty());
    assert_eq!(spool.dropped(), 2);
    assert_eq!(
        contents(dir.path()),
        before,
        "the rewritten file is replaced by the one from before the push"
    );

    // b is still on disk, so a restart sends it once: at least once, as documented.
    let mut reopened = Spool::open(dir.path()).unwrap().with_limits(1, MAX_BYTES);
    assert_eq!(targets(&reopened), [one("b")]);
    assert_eq!(
        reopened.dropped(),
        1,
        "the drop that never reached the metadata"
    );
    assert_eq!(
        reopened.push(vec![signal("d")]).unwrap(),
        2,
        "the failed batch's number is reused"
    );
}

#[test]
fn a_failed_metadata_save_after_creating_the_file_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();

    spool.inject(&[Stage::SaveMeta]);
    assert!(spool.push(vec![signal("a")]).is_err());

    assert!(spool.is_empty());
    assert!(
        !spool_file(dir.path()).exists(),
        "a file the failed push created is removed"
    );
    let mut reopened = Spool::open(dir.path()).unwrap();
    assert!(reopened.is_empty());
    assert_eq!(
        reopened.push(vec![signal("b")]).unwrap(),
        0,
        "the first batch keeps the first number"
    );
    assert_eq!(seqs(&Spool::open(dir.path()).unwrap()), [0]);
}

#[test]
fn a_failed_undo_keeps_the_batch_out_of_memory_and_reports_uncertainty() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap();
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);

    // The metadata cannot be saved and the truncation fails too, as on a failing disk.
    spool.inject(&[Stage::SaveMeta, Stage::Restore]);
    assert!(spool.push(vec![signal("b")]).is_err());

    // The live spool never sends a batch whose commit status is uncertain.
    assert_eq!(targets(&spool), [one("a")]);
    assert_eq!(seqs(&spool), [0]);

    // The bytes are still in the file, so a restart reads them and sends them once.
    let reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("a"), one("b")]);
    assert_eq!(seqs(&reopened), [0, 1]);
    let mut reopened = reopened;
    assert_eq!(
        reopened.push(vec![signal("c")]).unwrap(),
        2,
        "the number after the resent batch"
    );
}
