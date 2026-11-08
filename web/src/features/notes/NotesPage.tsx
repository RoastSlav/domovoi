import { useState } from "react";
import { Link } from "react-router";
import { useNoteSearch, useNotes } from "../../api/hooks/notes";
import { useSelectedHost } from "../../app/HostContext";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { PageIntro } from "../../components/PageIntro";
import { RelativeTime } from "../../components/RelativeTime";
import ui from "../../components/ui.module.css";
import { useDebounced } from "../../components/useDebounced";
import { HostScope } from "./HostScope";
import { ImportDialog } from "./ImportDialog";
import styles from "./notes.module.css";

/** How long typing must pause before a search runs. */
export const SEARCH_DEBOUNCE_MS = 300;

/** The hub's own bookkeeping tags: they mark generated notes, and read as one word. */
const BOOKKEEPING_TAGS = ["generated", "host-profile"];

function tagLine(tags: string[]): string {
  const visible = tags.filter((tag) => !BOOKKEEPING_TAGS.includes(tag));
  if (tags.includes("generated")) {
    visible.push("Generated");
  }
  return visible.join(", ");
}

function SearchResults({ q, host }: { q: string; host: string | undefined }) {
  const hits = useNoteSearch(q, host);
  if (hits.isPending) {
    return <p>Searching…</p>;
  }
  if (hits.error) {
    return <EmptyState title="Search failed" />;
  }
  if (hits.data.length === 0) {
    return <EmptyState title={`No notes match “${q}”`} />;
  }
  return (
    <ul className={styles.list} aria-label="Search results">
      {hits.data.map((hit) => (
        <li key={hit.note.id} className={styles.item}>
          <Link to={`/notes/${hit.note.id}`}>{hit.note.title}</Link>
          <p>{hit.snippet}</p>
        </li>
      ))}
    </ul>
  );
}

function NoteList({ host }: { host: string | undefined }) {
  const notes = useNotes(host);
  if (notes.isPending) {
    return <p>Loading…</p>;
  }
  if (notes.error) {
    return <EmptyState title="Could not load notes" />;
  }
  if (notes.data.length === 0) {
    return (
      <EmptyState title="No notes here yet">
        <p>Write one, or import a folder of Markdown runbooks.</p>
      </EmptyState>
    );
  }
  return (
    <ul className={styles.list} aria-label="Notes">
      {notes.data.map((note) => (
        <li key={note.id} className={styles.item}>
          <Link to={`/notes/${note.id}`}>{note.title}</Link>
          <p>
            Updated <RelativeTime at={note.updated_at} />
            {tagLine(note.tags) !== "" && ` · ${tagLine(note.tags)}`}
          </p>
        </li>
      ))}
    </ul>
  );
}

/** Notes, global or per host: search, browse, write and import. Starts on the selected host. */
export function NotesPage() {
  const [selected] = useSelectedHost();
  // A pick made here lasts only while the sidebar's selection stays what it was.
  const [pick, setPick] = useState<{ base: string | undefined; host: string } | null>(null);
  const host = pick && pick.base === selected ? pick.host || undefined : selected;
  const [query, setQuery] = useState("");
  const settled = useDebounced(query.trim(), SEARCH_DEBOUNCE_MS);
  const [importing, setImporting] = useState(false);

  const setHost = (next: string) => setPick({ base: selected, host: next });

  return (
    <>
      <div className={ui.pageHead}>
        <h1>Notes</h1>
        <div className={styles.actions}>
          <Button onClick={() => setImporting(true)}>Import</Button>
          <Link
            className={`${ui.button} ${ui.primary}`}
            to={host ? `/notes/new?host=${encodeURIComponent(host)}` : "/notes/new"}
          >
            New note
          </Link>
        </div>
      </div>
      <PageIntro topic="notes" docs="guide/migrating.html#4-bring-your-notes">
        Runbooks in Markdown: how your setup works and how you fixed things before. Triage searches
        them and gives the relevant ones to the model.
      </PageIntro>
      <div className={styles.toolbar} role="search">
        <div className={`${styles.field} ${styles.search}`}>
          <label htmlFor="notes-search">Search</label>
          <input
            id="notes-search"
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Words in a title or body"
          />
        </div>
        <div className={styles.field}>
          <label htmlFor="notes-host">Notes about</label>
          <HostScope id="notes-host" value={host ?? ""} onChange={setHost} />
        </div>
      </div>
      {settled ? <SearchResults q={settled} host={host} /> : <NoteList host={host} />}
      {importing && <ImportDialog host={host} onClose={() => setImporting(false)} />}
    </>
  );
}
