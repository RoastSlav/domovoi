//! Fault injection for `Spool::push`: every failure gives its sequence number back, puts the
//! spool file back the way it was, and never leaves a batch the caller was told had failed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use domovoi_core::{CheckKey, HostId, Severity, Signal, SourceKind, TargetKey};
use domovoi_testkit::build::T0;

use super::{MAX_BYTES, SPOOL_FILE, SPOOL_META_FILE, Spool, Stage};

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

#[test]
fn a_snapshot_read_failure_aborts_before_rewriting() {
    let dir = tempfile::tempdir().unwrap();
    // One batch fits, so the next push shrinks and has to rewrite the whole file.
    let mut spool = Spool::open(dir.path()).unwrap().with_limits(1, MAX_BYTES);
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    assert_eq!(spool.push(vec![signal("b")]).unwrap(), 1);
    let file_before = contents(dir.path());
    let meta_before = std::fs::read(dir.path().join(SPOOL_META_FILE)).unwrap();
    let bytes_before = spool.bytes;
    let dropped_before = spool.dropped();
    assert!(bytes_before > 0, "b is pending before the failed push");

    // Without a valid snapshot of the file there is no way back, so the push must not rewrite it.
    spool.inject(&[Stage::Snapshot]);
    assert!(spool.push(vec![signal("c")]).is_err());

    assert_eq!(
        contents(dir.path()),
        file_before,
        "the file is not rewritten without a snapshot"
    );
    assert_eq!(
        std::fs::read(dir.path().join(SPOOL_META_FILE)).unwrap(),
        meta_before,
        "the metadata is not touched"
    );
    // `shrink` dropped b on the way in, which is its documented effect on any over-limit push;
    // the failed batch itself is gone and its bytes are not counted.
    assert!(spool.pending_after(None).is_empty());
    assert_eq!(spool.dropped(), dropped_before + 1);
    assert_eq!(spool.bytes, 0);

    // A restart reads the file exactly as it was before the failed push.
    let mut reopened = Spool::open(dir.path()).unwrap().with_limits(1, MAX_BYTES);
    assert_eq!(targets(&reopened), [one("b")]);
    assert_eq!(seqs(&reopened), [1]);
    assert_eq!(
        reopened.push(vec![signal("d")]).unwrap(),
        2,
        "the failed batch's number is reused"
    );
}

/// Records that an error-level event was emitted on the thread it is set on.
struct LogProbe(Arc<AtomicBool>);

impl tracing::Subscriber for LogProbe {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

#[test]
fn a_failed_restore_after_rewrite_keeps_the_batch_out_of_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut spool = Spool::open(dir.path()).unwrap().with_limits(1, MAX_BYTES);
    assert_eq!(spool.push(vec![signal("a")]).unwrap(), 0);
    assert_eq!(spool.push(vec![signal("b")]).unwrap(), 1);

    // The rewrite lands, the metadata fails, and putting the file back fails too.
    let logged = Arc::new(AtomicBool::new(false));
    let probe = tracing::Dispatch::new(LogProbe(logged.clone()));
    spool.inject(&[Stage::SaveMeta, Stage::Restore]);
    let pushed = {
        let _default = tracing::dispatcher::set_default(&probe);
        spool.push(vec![signal("c")])
    };
    assert!(pushed.is_err());
    assert!(
        logged.load(Ordering::SeqCst),
        "the failed restore is logged at error"
    );

    // The live spool never sends a batch whose commit status is uncertain, and its number is
    // free again for the next push.
    assert!(spool.pending_after(None).is_empty());
    assert_eq!(spool.meta.next_seq, 2, "the failed number is restored");
    assert_eq!(
        spool.dropped(),
        2,
        "both batches shrink dropped are counted"
    );

    // The rewrite reached the disk and could not be undone there: a restart reads the batch once
    // and resends it. This is the documented uncertain-compensation outcome, not a rollback.
    let reopened = Spool::open(dir.path()).unwrap();
    assert_eq!(targets(&reopened), [one("c")]);
    assert_eq!(seqs(&reopened), [2]);
    assert_eq!(
        reopened.dropped(),
        1,
        "the drop that never reached the metadata"
    );
}
