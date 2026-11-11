import { Link } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import type { Note } from "../../api/types";
import {
  Cluster,
  DataTable,
  Highlight,
  RelativeTime,
  Tag,
  Text,
  type Column,
} from "../../components";
import { excerptOf, visibleTags } from "./excerpt";
import styles from "./notes.module.css";

/** One line of the notes table: a note and the text shown under its title. */
export interface NoteRow {
  excerpt: string;
  note: Note;
}

/** Rows for a plain list: the excerpt is the start of each note's body. */
export function listRows(notes: readonly Note[]): NoteRow[] {
  return notes.map((note) => ({ note, excerpt: excerptOf(note.body_markdown) }));
}

function Scope({ note }: { note: Note }) {
  const names = useHostNames();
  const name = note.host_id ? names.get(note.host_id) : undefined;
  return <Tag>{note.host_id ? (name ?? "A host") : "Global"}</Tag>;
}

function Tags({ note }: { note: Note }) {
  const tags = visibleTags(note.tags);
  const generated = note.tags.includes("generated");
  if (tags.length === 0 && !generated) {
    return <Text muted>–</Text>;
  }
  return (
    <Cluster gap={1}>
      {tags.map((tag) => (
        <Tag key={tag}>{tag}</Tag>
      ))}
      {generated && <Tag tone="info">Generated</Tag>}
    </Cluster>
  );
}

function columns(query: string): Column<NoteRow>[] {
  return [
    {
      key: "title",
      label: "Note",
      cell: ({ note, excerpt }) => (
        <div className={styles.title}>
          <Link to={`/notes/${note.id}`}>
            <Highlight text={note.title} query={query} />
          </Link>
          {excerpt && (
            <Text muted size="sm">
              <span className={styles.excerpt}>
                <Highlight text={excerpt} query={query} />
              </span>
            </Text>
          )}
        </div>
      ),
    },
    { key: "scope", label: "About", cell: ({ note }) => <Scope note={note} /> },
    { key: "tags", label: "Tags", cell: ({ note }) => <Tags note={note} /> },
    {
      key: "updated",
      label: "Updated",
      cell: ({ note }) => <RelativeTime at={note.updated_at} />,
    },
  ];
}

/**
 * Notes as a table: the title (a link) with a two-line excerpt, which host they are about,
 * their tags and when they changed. `query` marks the matching words in a search result.
 */
export function NotesTable({
  caption,
  rows,
  query = "",
}: {
  caption: string;
  query?: string;
  rows: readonly NoteRow[];
}) {
  return (
    <DataTable
      caption={caption}
      columns={columns(query)}
      rows={rows}
      rowKey={({ note }) => note.id}
    />
  );
}
