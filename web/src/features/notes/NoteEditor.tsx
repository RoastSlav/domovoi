import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { useCreateNote, useDeleteNote, useSaveNote } from "../../api/hooks/notes";
import type { Note } from "../../api/types";
import {
  Button,
  Card,
  ConfirmDialog,
  ErrorDetail,
  Field,
  Grid,
  InlineMessage,
  Markdown,
  PageHeader,
  SegmentedControl,
  Section,
  Stack,
  Tag,
  Text,
  Textarea,
  TextInput,
  useActionFeedback,
} from "../../components";
import { HostScope } from "./HostScope";
import { draftOf, inputOf, sameDraft, type Draft } from "./noteDraft";
import { SaveButton } from "./SaveButton";
import styles from "./notes.module.css";
import { useLeaveGuard } from "./useLeaveGuard";

/**
 * The note form: title, host, tags, and the body beside its preview (a Write and Preview
 * switch on a phone). Leaving with unsaved changes asks first; Ctrl or Cmd with S saves.
 */
export function NoteEditor({ note, host: initialHost }: { host: string; note: Note | undefined }) {
  const [draft, setDraft] = useState(() => draftOf(note, initialHost));
  const [saved, setSaved] = useState(draft);
  const [mode, setMode] = useState<"write" | "preview">("write");
  const [deleting, setDeleting] = useState(false);
  const navigate = useNavigate();
  const dirty = !sameDraft(draft, saved);
  const { guard, allowLeaving } = useLeaveGuard(dirty);
  const set = (change: Partial<Draft>) => setDraft((now) => ({ ...now, ...change }));

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
  const error = create.error ?? save.error;
  const busy = create.isPending || save.isPending;

  const reason = !draft.title.trim()
    ? "Give the note a title first."
    : note && !dirty
      ? "There are no changes to save."
      : undefined;

  const submit = () => {
    if (reason || busy) {
      return;
    }
    const sent = draft;
    if (note) {
      save.mutate({ id: note.id, body: inputOf(sent) }, { onSuccess: () => setSaved(sent) });
    } else {
      create.mutate(inputOf(sent), {
        onSuccess: (created) => {
          allowLeaving();
          void navigate(`/notes/${created.id}`, { replace: true });
        },
      });
    }
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        submit();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  return (
    <>
      <Text block>
        <Link to="/notes">Back to notes</Link>
      </Text>
      <PageHeader
        title={note?.title ?? "New note"}
        meta={dirty && <Tag tone="warning">Unsaved changes</Tag>}
        actions={
          <>
            {note && (
              <Button variant="danger" icon="trash" onClick={() => setDeleting(true)}>
                Delete
              </Button>
            )}
            <SaveButton busy={busy} reason={reason} onSave={submit} />
          </>
        }
      />
      <form
        aria-label="Note"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <Stack gap={4}>
          <Card label="Details">
            <Grid min="sm" gap={4}>
              <Field label="Title" required>
                <TextInput value={draft.title} onChange={(e) => set({ title: e.target.value })} />
              </Field>
              <HostScope label="About" value={draft.host} onChange={(host) => set({ host })} />
              <Field label="Tags" hint="Separate tags with commas.">
                <TextInput value={draft.tags} onChange={(e) => set({ tags: e.target.value })} />
              </Field>
            </Grid>
          </Card>
          {error && (
            <InlineMessage tone="danger" title={error.title}>
              <ErrorDetail error={error} />
            </InlineMessage>
          )}
          <div className={styles.switch}>
            <SegmentedControl
              label="Editor view"
              value={mode}
              onChange={setMode}
              options={[
                { value: "write", label: "Write", icon: "edit" },
                { value: "preview", label: "Preview", icon: "eye" },
              ]}
            />
          </div>
          <div className={styles.panes} data-mode={mode}>
            <div className={`${styles.pane} ${styles.writePane}`}>
              <Field label="Body (Markdown)">
                <Textarea
                  mono
                  className={styles.editorArea}
                  value={draft.body}
                  onChange={(e) => set({ body: e.target.value })}
                />
              </Field>
            </div>
            <div className={`${styles.pane} ${styles.previewPane}`}>
              <Section title="Preview">
                <Card>
                  <div className={styles.preview}>
                    {draft.body.trim() ? (
                      <Markdown text={draft.body} />
                    ) : (
                      <Text muted>Nothing to preview yet. Write something in the body.</Text>
                    )}
                  </div>
                </Card>
              </Section>
            </div>
          </div>
        </Stack>
      </form>
      {guard}
      {deleting && note && (
        <ConfirmDialog
          title={`Delete “${note.title}”?`}
          consequence="Triage will no longer find it. This cannot be undone."
          confirmLabel="Delete note"
          tone="danger"
          busy={remove.isPending}
          error={remove.error}
          onConfirm={() =>
            remove.mutate(note.id, {
              onSuccess: () => {
                allowLeaving();
                void navigate("/notes", { replace: true });
              },
            })
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
