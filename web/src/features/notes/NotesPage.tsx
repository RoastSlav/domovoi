import { useState } from "react";
import { useNoteSearch, useNotes } from "../../api/hooks/notes";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  LinkButton,
  PageHeader,
  QueryBoundary,
  Skeleton,
  TextInput,
  useDebounced,
} from "../../components";
import { HostScope } from "./HostScope";
import { ImportDialog } from "./ImportDialog";
import { listRows, NotesTable } from "./NotesTable";
import styles from "./notes.module.css";

/** How long typing must pause before a search runs. */
export const SEARCH_DEBOUNCE_MS = 300;

function SearchResults({
  q,
  host,
  onClear,
}: {
  host: string | undefined;
  onClear: () => void;
  q: string;
}) {
  const hits = useNoteSearch(q, host);
  return (
    <QueryBoundary
      query={hits}
      context="search the notes"
      loading={<Skeleton shape="rows" count={4} />}
      isEmpty={(data) => data.length === 0}
      empty={
        <EmptyState
          icon="search"
          title={`No notes match “${q}”`}
          action={<Button onClick={onClear}>Clear search</Button>}
        >
          <p>Try fewer or different words.</p>
        </EmptyState>
      }
    >
      {(data) => (
        <NotesTable
          caption="Search results"
          query={q}
          rows={data.map((hit) => ({ note: hit.note, excerpt: hit.snippet }))}
        />
      )}
    </QueryBoundary>
  );
}

function NoteList({ host }: { host: string | undefined }) {
  const notes = useNotes(host);
  return (
    <QueryBoundary
      query={notes}
      context="load notes"
      loading={<Skeleton shape="rows" count={5} />}
      isEmpty={(data) => data.length === 0}
      empty={
        <EmptyState icon="note" title="No notes here yet">
          <p>Write one, or import a folder of Markdown runbooks.</p>
        </EmptyState>
      }
    >
      {(data) => <NotesTable caption="Notes" rows={listRows(data)} />}
    </QueryBoundary>
  );
}

/** Notes, global or per host: search, browse, write and import. Starts on the selected host. */
export function NotesPage() {
  const [selected] = useSelectedHost();
  // A pick made here lasts only while the sidebar's selection stays what it was.
  const [pick, setPick] = useState<{ base: string | undefined; host: string } | null>(null);
  const host = pick && pick.base === selected ? pick.host || undefined : selected;
  const [query, setQuery] = useState("");
  const debounced = useDebounced(query.trim(), SEARCH_DEBOUNCE_MS);
  // Emptying the box goes back to the list at once; only typing waits.
  const settled = query.trim() === "" ? "" : debounced;
  const [importing, setImporting] = useState(false);

  const setHost = (next: string) => setPick({ base: selected, host: next });

  return (
    <>
      <PageHeader
        title="Notes"
        docs="guide/migrating.html#4-bring-your-notes"
        intro="Markdown runbooks that triage searches and hands to the model."
        actions={
          <>
            <Button icon="plus" onClick={() => setImporting(true)}>
              Import
            </Button>
            <LinkButton
              variant="primary"
              icon="note"
              to={host ? `/notes/new?host=${encodeURIComponent(host)}` : "/notes/new"}
            >
              New note
            </LinkButton>
          </>
        }
      />
      <div className={styles.toolbar} role="search">
        <div className={styles.search}>
          <TextInput
            type="search"
            aria-label="Search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search titles and bodies"
          />
        </div>
        <HostScope bare label="Notes about" value={host ?? ""} onChange={setHost} />
      </div>
      {settled ? (
        <SearchResults q={settled} host={host} onClear={() => setQuery("")} />
      ) : (
        <NoteList host={host} />
      )}
      {importing && <ImportDialog host={host} onClose={() => setImporting(false)} />}
    </>
  );
}
