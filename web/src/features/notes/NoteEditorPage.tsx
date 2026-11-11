import { Link, useParams, useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useNote } from "../../api/hooks/notes";
import { ApiFailure } from "../../api/types";
import { usePageName } from "../../app/PageTitle";
import { EmptyState, LoadError, Skeleton, Stack } from "../../components";
import { NoteEditor } from "./NoteEditor";

/** The editor's shape while the note loads. */
function EditorSkeleton() {
  return (
    <Stack gap={3}>
      <Skeleton shape="line" count={2} />
      <Skeleton shape="block" />
    </Stack>
  );
}

/** Writing a new note (`/notes/new`) or editing one, with a live preview. */
export function NoteEditorPage() {
  const { id = "" } = useParams();
  const [params] = useSearchParams();
  const isNew = id === "new";
  const note = useNote(isNew ? "" : id);
  const hostNames = useHostNames();
  usePageName(isNew ? undefined : note.data?.title);

  if (!isNew && note.data === undefined) {
    if (note.isPending) {
      return <EditorSkeleton />;
    }
    if (!note.error || (note.error instanceof ApiFailure && note.error.status === 404)) {
      return (
        <EmptyState title="No such note" action={<Link to="/notes">Back to notes</Link>}>
          <p>It may have been deleted, or the address is wrong.</p>
        </EmptyState>
      );
    }
    return <LoadError error={note.error} context="load the note" onRetry={note.refetch} />;
  }
  const host = isNew
    ? (params.get("host") ?? "")
    : note.data?.host_id
      ? (hostNames.get(note.data.host_id) ?? "")
      : "";
  return (
    <NoteEditor
      key={`${note.data?.id ?? "new"}-${host}`}
      note={isNew ? undefined : note.data}
      host={host}
    />
  );
}
