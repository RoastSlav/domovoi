//! Notes and note search.
//!
//! FTS5 finds the notes holding every query word. They are ranked by how often the words
//! occur, with a title occurrence worth [`TITLE_WEIGHT`] body or tag occurrences, so title
//! matches come first; FTS5 `bm25` (same weights) breaks ties. `bm25` alone is unreliable
//! on a small knowledge base: once a word is in more than half the notes its IDF drops to
//! nearly zero and every match scores the same.

use async_trait::async_trait;
use domovoi_core::ports::{NoteHit, NoteRepo, Page, StoreError};
use domovoi_core::{HostId, Note, NoteId};

use crate::codec::{from_json, from_ms, json, ms, parse};
use crate::db::Store;
use crate::error::db;
use crate::fts::{match_all, words};

/// Longest snippet, in characters.
pub const SNIPPET_CHARS: usize = 160;
/// How many body or tag occurrences one title occurrence is worth.
pub const TITLE_WEIGHT: f64 = 10.0;
/// Most candidates ranked per search.
pub const MAX_CANDIDATES: i64 = 500;

macro_rules! note_columns {
    () => {
        "SELECT id, host_id, title, body, tags, updated_at FROM notes"
    };
}

#[derive(sqlx::FromRow)]
struct NoteRow {
    id: String,
    host_id: Option<String>,
    title: String,
    body: String,
    tags: String,
    updated_at: i64,
}

impl NoteRow {
    fn into_note(self) -> Result<Note, StoreError> {
        Ok(Note {
            id: parse(&self.id, "note id")?,
            host: self
                .host_id
                .as_deref()
                .map(|h| parse(h, "host id"))
                .transpose()?,
            title: self.title,
            body_markdown: self.body,
            tags: from_json(&self.tags, "note tags")?,
            updated_at: from_ms(self.updated_at)?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct HitRow {
    #[sqlx(flatten)]
    note: NoteRow,
    score: f64,
    snippet: String,
}

fn shorten(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(SNIPPET_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

#[async_trait]
impl NoteRepo for Store {
    async fn upsert(&self, note: &Note) -> Result<(), StoreError> {
        const SQL: &str = "INSERT INTO notes (id, host_id, title, body, tags, updated_at) \
            VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (id) DO UPDATE SET host_id = excluded.host_id, \
            title = excluded.title, body = excluded.body, tags = excluded.tags, \
            updated_at = excluded.updated_at";
        sqlx::query(SQL)
            .bind(note.id.to_string())
            .bind(note.host.map(|h| h.to_string()))
            .bind(&note.title)
            .bind(&note.body_markdown)
            .bind(json(&note.tags)?)
            .bind(ms(note.updated_at))
            .execute(&self.write)
            .await
            .map_err(db)?;
        Ok(())
    }

    async fn get(&self, id: NoteId) -> Result<Option<Note>, StoreError> {
        const SQL: &str = concat!(note_columns!(), " WHERE id = ?");
        let row: Option<NoteRow> = sqlx::query_as(SQL)
            .bind(id.to_string())
            .fetch_optional(&self.read)
            .await
            .map_err(db)?;
        row.map(NoteRow::into_note).transpose()
    }

    async fn delete(&self, id: NoteId) -> Result<(), StoreError> {
        let done = sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.write)
            .await
            .map_err(db)?;
        if done.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn list(&self, host: Option<HostId>) -> Result<Vec<Note>, StoreError> {
        const SQL: &str = concat!(
            note_columns!(),
            " WHERE host_id IS ? ORDER BY updated_at DESC, id DESC"
        );
        let rows: Vec<NoteRow> = sqlx::query_as(SQL)
            .bind(host.map(|h| h.to_string()))
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        rows.into_iter().map(NoteRow::into_note).collect()
    }

    async fn search(
        &self,
        query: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<NoteHit>, StoreError> {
        const SQL: &str = "SELECT n.id, n.host_id, n.title, n.body, n.tags, n.updated_at, \
            -bm25(notes_fts, 10.0, 1.0, 1.0) AS score, \
            snippet(notes_fts, 1, '', '', '…', 16) AS snippet \
            FROM notes_fts JOIN notes n ON n.rowid = notes_fts.rowid \
            WHERE notes_fts MATCH ?1 AND (?2 IS NULL OR n.host_id IS NULL OR n.host_id = ?2) \
            ORDER BY score DESC LIMIT ?3";
        let Some(fts) = match_all(query) else {
            return Ok(Vec::new());
        };
        let rows: Vec<HitRow> = sqlx::query_as(SQL)
            .bind(fts)
            .bind(host.map(|h| h.to_string()))
            .bind(MAX_CANDIDATES)
            .fetch_all(&self.read)
            .await
            .map_err(db)?;
        let wanted = words(query);
        let mut hits = rows
            .into_iter()
            .map(|row| hit(row, &wanted))
            .collect::<Result<Vec<_>, _>>()?;
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(b.note.updated_at.cmp(&a.note.updated_at))
                .then(b.note.id.cmp(&a.note.id))
        });
        hits.truncate(usize::from(limit.clamp(1, Page::MAX_LIMIT)));
        Ok(hits)
    }
}

/// How often `wanted` words occur in `text`.
fn occurrences(text: &str, wanted: &[String]) -> f64 {
    let count = words(text).iter().filter(|w| wanted.contains(w)).count();
    count as f64
}

/// Squashes a non-negative `bm25` into `[0, 1)`, so it only breaks ties between equal
/// counts.
fn tie_breaker(bm25: f64) -> f64 {
    let b = bm25.max(0.0);
    b / (1.0 + b)
}

fn hit(row: HitRow, wanted: &[String]) -> Result<NoteHit, StoreError> {
    let snippet = match shorten(&row.snippet) {
        s if s.trim_matches('…').trim().is_empty() => shorten(&row.note.title),
        s => s,
    };
    let note = row.note.into_note()?;
    let count = TITLE_WEIGHT * occurrences(&note.title, wanted)
        + occurrences(&note.body_markdown, wanted)
        + note
            .tags
            .iter()
            .map(|t| occurrences(t, wanted))
            .sum::<f64>();
    Ok(NoteHit {
        snippet: if snippet.is_empty() {
            "…".to_owned()
        } else {
            snippet
        },
        score: count + tie_breaker(row.score),
        note,
    })
}
