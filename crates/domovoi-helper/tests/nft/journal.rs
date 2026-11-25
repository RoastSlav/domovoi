//! The write-ahead journal and crash recovery.

use std::sync::{Arc, Mutex};

use domovoi_core::containment::{JournalOp, JournalRecord};
use domovoi_helper::nft::journal_file::{Journal, JournalError, Phase, fold};
use domovoi_helper::nft::{apply, recover};
use jiff::Timestamp;
use uuid::Uuid;

use super::probe::{count, seed};
use super::world::{OPEN, World, facts, no_sys, state, unverified};

const HOST: &str = "203.0.113.7/32";

fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_767_225_600 + secs).expect("timestamp")
}

fn intent(block: &str, op: JournalOp) -> JournalRecord {
    JournalRecord::Intent {
        op,
        block: block.to_owned(),
        source: HOST.to_owned(),
        set: "blocked4".to_owned(),
        ttl_s: 600,
        at: at(0),
    }
}

#[tokio::test]
async fn journal_record_is_written_before_the_nft_call() {
    let seen = Arc::new(Mutex::new(Vec::<bool>::new()));
    let dir_cell: Arc<Mutex<Option<std::path::PathBuf>>> = Arc::new(Mutex::new(None));
    let (seen2, dir2) = (seen.clone(), dir_cell.clone());
    let hook: super::world::Hook = Arc::new(move |spec| {
        let creates = spec
            .stdin
            .as_deref()
            .is_some_and(|s| s.starts_with("create element"));
        if creates && let Some(dir) = dir2.lock().expect("dir").clone() {
            let text = std::fs::read_to_string(dir.join("containment.journal")).unwrap_or_default();
            seen2
                .lock()
                .expect("seen")
                .push(text.contains("\"rec\":\"intent\""));
        }
    });
    let w = World::with_hook(Some(hook));
    *dir_cell.lock().expect("dir") = Some(w.path().to_path_buf());
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    apply::apply(&cx, &st, Uuid::from_u128(1), HOST, 600, Some(&unverified()))
        .await
        .expect("apply");
    assert_eq!(
        *seen.lock().expect("seen"),
        [true],
        "the intent line preceded the create"
    );
}

#[test]
fn torn_final_journal_line_is_ignored_and_corrupt_middle_refuses() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let journal = Journal::new(w.path());
    let path = w.path().join("containment.journal");
    let good = intent("a", JournalOp::Apply).to_line().expect("line");
    std::fs::write(&path, format!("{good}\n{{\"rec\":\"app")).expect("write");
    let loaded = journal.load().expect("a torn final line is tolerated");
    assert!(loaded.torn);
    assert_eq!(loaded.records.len(), 1);
    journal
        .append(
            &guard,
            &JournalRecord::Noop {
                block: "a".to_owned(),
            },
        )
        .expect("append after a torn tail");
    let again = journal.load().expect("the file is whole again");
    assert!(!again.torn);
    assert_eq!(again.records.len(), 2);
    std::fs::write(&path, format!("{good}\nnot json\n{good}\n")).expect("write");
    assert!(matches!(journal.load(), Err(JournalError::Corrupt)));
}

#[test]
fn compaction_drops_blocks_finished_more_than_a_day_ago() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let journal = Journal::new(w.path());
    for r in [
        intent("old", JournalOp::Apply),
        JournalRecord::Applied {
            block: "old".to_owned(),
            at: at(0),
        },
        JournalRecord::RolledBack {
            block: "old".to_owned(),
            at: at(10),
        },
        intent("new", JournalOp::Apply),
        JournalRecord::Applied {
            block: "new".to_owned(),
            at: at(2 * 86_400),
        },
    ] {
        journal.append(&guard, &r).expect("append");
    }
    let loaded = journal.load().expect("load");
    let changed = journal
        .compact(&guard, &loaded, at(2 * 86_400 + 5))
        .expect("compact");
    assert!(changed);
    let kept = fold(&journal.load().expect("load").records);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].block, "new");
    assert_eq!(kept[0].phase, Phase::Applied);
}

#[tokio::test]
async fn a_dangling_intent_is_settled_from_membership() {
    let w = World::new();
    let lock = w.lock();
    let guard = lock.acquire().expect("lock");
    let sys = no_sys();
    let cx = w.cx(&guard, &sys);
    let st = state(&cx, facts(OPEN)).await;
    cx.journal
        .append(&guard, &intent("present", JournalOp::Apply))
        .expect("append");
    cx.journal
        .append(
            &guard,
            &JournalRecord::Intent {
                op: JournalOp::Apply,
                block: "absent".to_owned(),
                source: "203.0.113.99/32".to_owned(),
                set: "blocked4".to_owned(),
                ttl_s: 600,
                at: at(0),
            },
        )
        .expect("append");
    seed(&w, "blocked4", "203.0.113.7", 20);
    let st = state(&cx, facts(OPEN)).await;
    let _ = count(&w, "blocked4");
    let changed = recover::recover(&cx, &st.ens.view, &st.loaded).expect("recover");
    assert!(changed);
    let blocks = fold(&cx.journal.load().expect("load").records);
    let phase = |id: &str| blocks.iter().find(|b| b.block == id).expect(id).phase;
    assert_eq!(phase("present"), Phase::Applied);
    assert_eq!(phase("absent"), Phase::Failed);
    let _ = st;
}
