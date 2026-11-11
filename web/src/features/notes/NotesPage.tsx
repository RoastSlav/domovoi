import { useState } from "react";
import { Link } from "react-router";
import { useNoteSearch, useNotes } from "../../api/hooks/notes";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  LinkButton,
  LoadError,
  Loading,
  PageHeader,
  RelativeTime,
  useDebounced,
} from "../../components";
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
    return <Loading label="Searching…" />;
  }
  if (hits.error) {
    return <LoadError error={hits.error} context="search the notes" onRetry={hits.refetch} />;
  }
  if (hits.data.length === 0) {
    return (
      <EmptyState icon="search" title={`No notes match “${q}”`}>
        <p>Try fewer or different words.</p>
      </EmptyState>
    );
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
    return <Loading />;
  }
  if (notes.error) {
    return <LoadError error={notes.error} context="load notes" onRetry={notes.refetch} />;
  }
  if (notes.data.length === 0) {
    return (
      <EmptyState icon="note" title="No notes here yet">
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
      <PageHeader
        title="Notes"
        docs="guide/migrating.html#4-bring-your-notes"
        intro="Runbooks in Markdown: how your setup works and how you fixed things before. Triage searches them and gives the relevant ones to the model."
        actions={
          <>
            <Button onClick={() => setImporting(true)}>Import</Button>
            <LinkButton
              variant="primary"
              icon="plus"
              to={host ? `/notes/new?host=${encodeURIComponent(host)}` : "/notes/new"}
            >
              New note
            </LinkButton>
          </>
        }
      />
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
