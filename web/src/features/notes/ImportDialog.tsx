import { useState } from "react";
import { useImportNotes } from "../../api/hooks/notes";
import { Button, Dialog, InlineError, InlineMessage } from "../../components";
import { HostScope } from "./HostScope";
import styles from "./notes.module.css";

/** The most files one import takes, as the hub enforces. */
export const MAX_IMPORT_FILES = 200;

/** The Markdown files among `list`; folders bring images and other files along. */
export function markdownFiles(list: FileList | null): File[] {
  return [...(list ?? [])].filter((file) => file.name.toLowerCase().endsWith(".md"));
}

/** Imports a folder (or a selection) of Markdown files as notes. */
export function ImportDialog({
  host: initialHost,
  onClose,
}: {
  host: string | undefined;
  onClose: () => void;
}) {
  const [host, setHost] = useState(initialHost ?? "");
  const [files, setFiles] = useState<File[]>([]);
  const importNotes = useImportNotes();
  const result = importNotes.data;
  const tooMany = files.length > MAX_IMPORT_FILES;

  const pick = (list: FileList | null) => {
    importNotes.reset();
    setFiles(markdownFiles(list));
  };

  return (
    <Dialog
      title="Import notes"
      onClose={onClose}
      wide
      actions={
        <>
          <Button onClick={onClose}>{result ? "Done" : "Cancel"}</Button>
          {!result && (
            <Button
              variant="primary"
              loading={importNotes.isPending}
              disabledReason={
                files.length === 0
                  ? "Pick Markdown files first."
                  : tooMany
                    ? `Pick at most ${MAX_IMPORT_FILES} files at a time.`
                    : undefined
              }
              onClick={() => importNotes.mutate({ files, host: host || undefined })}
            >
              {`Import ${files.length} files`}
            </Button>
          )}
        </>
      }
    >
      <div className={styles.form}>
        <p>
          Each Markdown file becomes a note titled by its first <code># heading</code>, or its file
          name. Files already imported are skipped.
        </p>
        <div className={styles.field}>
          <label htmlFor="import-folder">Folder</label>
          <input
            id="import-folder"
            type="file"
            multiple
            accept=".md,text/markdown"
            {...{ webkitdirectory: "" }}
            onChange={(e) => pick(e.target.files)}
          />
        </div>
        <div className={styles.field}>
          <label htmlFor="import-files">Or pick files</label>
          <input
            id="import-files"
            type="file"
            multiple
            accept=".md,text/markdown"
            onChange={(e) => pick(e.target.files)}
          />
        </div>
        <div className={styles.field}>
          <label htmlFor="import-host">Notes about</label>
          <HostScope id="import-host" value={host} onChange={setHost} />
        </div>
        <p role="status">
          {files.length} Markdown file{files.length === 1 ? "" : "s"} selected.
        </p>
        {tooMany && (
          <InlineMessage
            tone="danger"
            title={`Pick at most ${MAX_IMPORT_FILES} files at a time.`}
          />
        )}
        <InlineError error={importNotes.error} context="import the notes" />
        {result && (
          <div className={styles.result} role="status">
            <p>
              Created {result.created} note{result.created === 1 ? "" : "s"}, skipped{" "}
              {result.skipped}.
            </p>
            {result.skipped_files.length > 0 && (
              <ul aria-label="Skipped files">
                {result.skipped_files.map((file) => (
                  <li key={file.name}>
                    <code>{file.name}</code>: {file.reason}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>
    </Dialog>
  );
}
