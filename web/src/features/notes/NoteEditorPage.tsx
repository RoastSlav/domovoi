import { useState, type FormEvent } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useCreateNote, useDeleteNote, useNote, useSaveNote } from "../../api/hooks/notes";
import { ApiFailure, type Note, type NoteInput } from "../../api/types";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { EmptyState } from "../../components/EmptyState";
import { Markdown } from "../../components/Markdown";
import { HostScope } from "./HostScope";
import styles from "./notes.module.css";

function failureText(error: unknown): string {
  if (error instanceof ApiFailure && error.status < 500) {
    return error.message;
  }
  return "Could not save the note. Try again.";
}

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
  const create = useCreateNote();
  const save = useSaveNote();
  const remove = useDeleteNote();
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
            <p className={styles.error} role="alert">
              {failureText(error)}
            </p>
          )}
          {save.isSuccess && !save.isPending && <p role="status">Saved.</p>}
          <div className={styles.actions}>
            <Button
              type="submit"
              variant="primary"
              disabled={!title.trim() || create.isPending || save.isPending}
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
          confirmLabel="Delete"
          confirmVariant="danger"
          busy={remove.isPending}
          onConfirm={() =>
            remove.mutate(note.id, { onSuccess: () => void navigate("/notes", { replace: true }) })
          }
          onCancel={() => setDeleting(false)}
        >
          <p>Triage will no longer find it. This cannot be undone.</p>
        </ConfirmDialog>
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

  if (!isNew && note.isPending) {
    return <p>Loading…</p>;
  }
  if (!isNew && (note.error || !note.data)) {
    return (
      <EmptyState title="No such note">
        <Link to="/notes">Back to notes</Link>
      </EmptyState>
    );
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
      <h1>{isNew ? "New note" : (note.data?.title ?? "Note")}</h1>
      <Editor
        key={`${note.data?.id ?? "new"}-${host}`}
        note={isNew ? undefined : note.data}
        host={host}
      />
    </>
  );
}
