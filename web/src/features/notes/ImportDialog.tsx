import { useState } from "react";
import { useImportNotes } from "../../api/hooks/notes";
import { Button, Dialog, DropZone, InlineError, InlineMessage, Stack } from "../../components";
import { HostScope } from "./HostScope";
import { ImportResult } from "./ImportResult";

/** The most files one import takes, as the hub enforces. */
export const MAX_IMPORT_FILES = 200;

/** The Markdown files among `files`; folders bring images and other files along. */
export function markdownFiles(files: readonly File[]): File[] {
  return files.filter((file) => file.name.toLowerCase().endsWith(".md"));
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
  const limit = `Pick at most ${MAX_IMPORT_FILES} files at a time.`;

  const pick = (chosen: File[]) => {
    importNotes.reset();
    setFiles(markdownFiles(chosen));
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
                files.length === 0 ? "Pick Markdown files first." : tooMany ? limit : undefined
              }
              onClick={() => importNotes.mutate({ files, host: host || undefined })}
            >
              {`Import ${files.length} files`}
            </Button>
          )}
        </>
      }
    >
      <Stack gap={3}>
        <DropZone
          label="Choose Markdown files or drop them here"
          hint="Each file becomes a note titled by its first heading, or its file name. Files already imported are skipped."
          accept=".md,text/markdown"
          folderLabel="Choose a folder instead"
          disabledReason={importNotes.isPending ? "Wait for the import to finish." : undefined}
          onFiles={pick}
        />
        <HostScope label="Notes about" value={host} onChange={setHost} />
        <p role="status">
          {files.length} Markdown file{files.length === 1 ? "" : "s"} selected.
        </p>
        {tooMany && <InlineMessage tone="danger" title={limit} />}
        <InlineError error={importNotes.error} context="import the notes" />
        {result && <ImportResult result={result} />}
      </Stack>
    </Dialog>
  );
}
