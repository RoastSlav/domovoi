import { useState, type FormEvent } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useCreateNote, useDeleteNote, useNote, useSaveNote } from "../../api/hooks/notes";
import { ApiFailure, type Note, type NoteInput } from "../../api/types";
import {
  Button,
  ConfirmDialog,
  EmptyState,
  ErrorDetail,
  InlineMessage,
  LoadError,
  Loading,
  Markdown,
  PageHeader,
  useActionFeedback,
} from "../../components";
import { HostScope } from "./HostScope";
import { usePageName } from "../../app/PageTitle";
import styles from "./notes.module.css";

function tagsOf(text: string): string[] {
  return text
    .split(",")
    .map((t) => t.trim())
    .filter(Boolean);
}

function Editor({ note, host: initialHost }: { note: Note | undefined; host: string }) {
  const [title, setTitle] = useState(note?.title ?? "");
  const [host, setHost] = useState(initialHost);
  const [tags, setTags] = useState(note?.tags.join(", ") ?? "");
  const [body, setBody] = useState(note?.body_markdown ?? "");
  const [deleting, setDeleting] = useState(false);
  const create = useActionFeedback(useCreateNote(), {
    success: "Note created",
    error: "save the note",
    errorDisplay: "inline",
  });
  const save = useActionFeedback(useSaveNote(), {
    success: "Note saved",
    error: "save the note",
    errorDisplay: "inline",
  });
  const remove = useActionFeedback(useDeleteNote(), {
    success: "Note deleted",
    error: "delete the note",
    errorDisplay: "inline",
  });
  const navigate = useNavigate();
  const error = create.error ?? save.error;

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const input: NoteInput = {
      title: title.trim(),
      body_markdown: body,
      tags: tagsOf(tags),
      ...(host ? { host } : {}),
    };
    if (note) {
      save.mutate({ id: note.id, body: input });
    } else {
      create.mutate(input, {
        onSuccess: (created) => void navigate(`/notes/${created.id}`, { replace: true }),
      });
    }
  };

  return (
    <>
      <div className={styles.editor}>
        <form className={styles.form} onSubmit={onSubmit} aria-label="Note">
          <div className={styles.field}>
            <label htmlFor="note-title">Title</label>
            <input id="note-title" value={title} onChange={(e) => setTitle(e.target.value)} />
          </div>
          <div className={styles.field}>
            <label htmlFor="note-host">About</label>
            <HostScope id="note-host" value={host} onChange={setHost} />
          </div>
          <div className={styles.field}>
            <label htmlFor="note-tags">Tags, comma-separated</label>
            <input id="note-tags" value={tags} onChange={(e) => setTags(e.target.value)} />
          </div>
          <div className={styles.field}>
            <label htmlFor="note-body">Body (Markdown)</label>
            <textarea id="note-body" value={body} onChange={(e) => setBody(e.target.value)} />
          </div>
          {error && (
            <InlineMessage tone="danger" title={error.title}>
              <ErrorDetail error={error} />
            </InlineMessage>
          )}
          <div className={styles.actions}>
            <Button
              type="submit"
              variant="primary"
              loading={create.isPending || save.isPending}
              disabledReason={title.trim() ? undefined : "Give the note a title first."}
            >
              Save
            </Button>
            {note && (
              <Button variant="danger" onClick={() => setDeleting(true)}>
                Delete
              </Button>
            )}
          </div>
        </form>
        <section className={styles.preview} aria-label="Preview">
          <h2>Preview</h2>
          <Markdown text={body} />
        </section>
      </div>
      {deleting && note && (
        <ConfirmDialog
          title={`Delete “${note.title}”?`}
          consequence="Triage will no longer find it. This cannot be undone."
          confirmLabel="Delete note"
          tone="danger"
          busy={remove.isPending}
          error={remove.error}
          onConfirm={() =>
            remove.mutate(note.id, { onSuccess: () => void navigate("/notes", { replace: true }) })
          }
          onCancel={() => {
            setDeleting(false);
            remove.reset();
          }}
        />
      )}
    </>
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

  if (!isNew && note.isPending) {
    return <Loading />;
  }
  if (!isNew && (note.error || !note.data)) {
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
    <>
      <p>
        <Link to="/notes">Notes</Link>
      </p>
      <PageHeader title={isNew ? "New note" : (note.data?.title ?? "Note")} />
      <Editor
        key={`${note.data?.id ?? "new"}-${host}`}
        note={isNew ? undefined : note.data}
        host={host}
      />
    </>
  );
}
