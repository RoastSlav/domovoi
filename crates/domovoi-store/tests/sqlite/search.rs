//! Full-text search: title matches rank first, and user text never reaches FTS5 as syntax.

use domovoi_core::ports::{IncidentRepo, NoteRepo};
use domovoi_core::{HostId, Note, NoteId};
use domovoi_testkit::build::{at, incident};

use crate::common::Fresh;

fn note(title: &str, body: &str, secs: i64) -> Note {
    Note {
        id: NoteId::new(),
        host: None,
        title: title.to_owned(),
        body_markdown: body.to_owned(),
        tags: Vec::new(),
        updated_at: at(secs),
    }
}

#[tokio::test]
async fn fts_search_ranks_title_matches_first_and_sanitises_operators() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    // Newest and with more body mentions, but only in the body.
    let body = note(
        "Rack wiring",
        "The backup job, then backup again, then backup.",
        9,
    );
    let titled = note("Backup schedule", "Runs nightly.", 1);
    let other = note("Power", "UPS shuts down the rack.", 5);
    for n in [&body, &titled, &other] {
        store.upsert(n).await.unwrap();
    }
    let hits = NoteRepo::search(store, "backup", None, 10).await.unwrap();
    let ids: Vec<NoteId> = hits.iter().map(|h| h.note.id).collect();
    assert_eq!(ids, [titled.id, body.id]);
    assert!(hits[0].score > hits[1].score);
    assert!(hits[1].snippet.contains("backup"), "{}", hits[1].snippet);

    let host = HostId::new();
    let zfs = incident(host, "zfs.pool_degraded", None).unwrap();
    IncidentRepo::insert(store, &zfs).await.unwrap();

    // FTS5 syntax in user text is plain words: no errors, no operators.
    for query in [
        "\"NEAR(\" OR",
        "NEAR(backup rack, 2)",
        "backup OR power",
        "backup*",
        "^backup",
        "-backup",
        "title:backup",
        "{title body}: backup",
        "\"",
        "'); DROP TABLE notes; --",
        "AND",
        "*",
    ] {
        let notes = NoteRepo::search(store, query, None, 10).await;
        assert!(notes.is_ok(), "{query}: {notes:?}");
        let incidents = IncidentRepo::search(store, query, 10).await;
        assert!(incidents.is_ok(), "{query}: {incidents:?}");
    }
    // `OR` is a word to find, not an operator, so nothing has all of these words.
    assert!(
        NoteRepo::search(store, "backup OR power", None, 10)
            .await
            .unwrap()
            .is_empty()
    );
    // `*` is not a prefix operator: `back` is not a word in any note.
    assert!(
        NoteRepo::search(store, "back*", None, 10)
            .await
            .unwrap()
            .is_empty()
    );
    // Column filters are ignored; the words still have to appear.
    let filtered = NoteRepo::search(store, "title:backup", None, 10)
        .await
        .unwrap();
    assert!(filtered.is_empty(), "\"title\" is a word to match");
    assert_eq!(
        IncidentRepo::search(store, "pool_degraded", 10)
            .await
            .unwrap()[0]
            .id(),
        zfs.id()
    );
    // The notes table is still there.
    assert_eq!(
        NoteRepo::search(store, "backup", None, 10)
            .await
            .unwrap()
            .len(),
        2
    );
}
