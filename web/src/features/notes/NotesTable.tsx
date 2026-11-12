import { useCallback } from "react";
import { Link, useNavigate } from "react-router";
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

/** How many tags show before the rest fold into a "+N" tag. */
const SHOWN_TAGS = 2;

/** Tags in one row: the first two, then "+N" with the others in its tooltip. */
function Tags({ note }: { note: Note }) {
  const all = [
    ...visibleTags(note.tags).map((tag) => ({ label: tag, generated: false })),
    ...(note.tags.includes("generated") ? [{ label: "Generated", generated: true }] : []),
  ];
  if (all.length === 0) {
    return <Text muted>–</Text>;
  }
  const rest = all.slice(SHOWN_TAGS);
  return (
    <Cluster gap={1} wrap={false}>
      {all.slice(0, SHOWN_TAGS).map((tag) => (
        <Tag key={tag.label} {...(tag.generated ? { tone: "info" as const } : {})}>
          {tag.label}
        </Tag>
      ))}
      {rest.length > 0 && (
        <Tag hint={rest.map((tag) => tag.label).join(", ")}>{`+${rest.length}`}</Tag>
      )}
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
 * Notes as a table; a click anywhere on a row opens it, and the title (a link) with a two-line excerpt, which host they are about,
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
  const navigate = useNavigate();
  const open = useCallback(({ note }: NoteRow) => void navigate(`/notes/${note.id}`), [navigate]);
  return (
    <DataTable
      caption={caption}
      columns={columns(query)}
      rows={rows}
      rowKey={({ note }) => note.id}
      onRowClick={open}
    />
  );
}
