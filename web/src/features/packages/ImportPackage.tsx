import { useState } from "react";
import { useImportPackage } from "../../api/hooks/packages";
import type { PackageSummary } from "../../api/types";
import {
  Button,
  DropZone,
  IconButton,
  InlineMessage,
  Stack,
  Text,
  Visible,
  useActionFeedback,
} from "../../components";
import { ImportResult } from "./ImportResult";
import styles from "./packages.module.css";

/** The part of a file name before its package suffix, for comparing the two files. */
function stem(file: File, suffix: string): string {
  return file.name.endsWith(suffix) ? file.name.slice(0, -suffix.length) : file.name;
}

function size(file: File): string {
  return file.size >= 1024 * 1024
    ? `${(file.size / (1024 * 1024)).toFixed(1)} MB`
    : `${Math.max(1, Math.round(file.size / 1024))} kB`;
}

/** A chosen file's name and size, with a way to take it back. */
function Chosen({ file, label, onRemove }: { file: File; label: string; onRemove: () => void }) {
  return (
    <div className={styles.fileRow}>
      <Text mono>{file.name}</Text>
      <Text muted size="sm">
        {size(file)}
      </Text>
      <IconButton icon="close" label={`Remove ${label}`} size="sm" onClick={onRemove} />
    </div>
  );
}

/**
 * Chooses an owner-signed archive and its detached signature file and imports them. The hub
 * is the authority on whether they match and are signed by a known key; this only sends them.
 * Not for installing: the result links to the install wizard.
 */
export function ImportPackage({ blockedReason }: { blockedReason: string | undefined }) {
  const [archive, setArchive] = useState<File | undefined>(undefined);
  const [signature, setSignature] = useState<File | undefined>(undefined);
  const [result, setResult] = useState<PackageSummary | undefined>(undefined);
  const importer = useActionFeedback(useImportPackage(), {
    error: "import the package",
    errorDisplay: "inline",
    success: () => null,
  });
  const mismatch =
    archive !== undefined &&
    signature !== undefined &&
    stem(archive, ".tar.zst") !== stem(signature, ".sig.toml");
  const reason =
    blockedReason ??
    (archive && signature ? undefined : "Choose the archive and its signature file.");

  const submit = () => {
    if (!archive || !signature) {
      return;
    }
    setResult(undefined);
    importer.mutate(
      { archive, signature },
      {
        onSuccess: (pkg) => {
          setResult(pkg);
          setArchive(undefined);
          setSignature(undefined);
        },
      },
    );
  };

  return (
    <Stack gap={4}>
      <Visible category="P1" id="packages.import.form">
        <Stack gap={3}>
          <div className={styles.zones}>
            <Stack gap={2}>
              <DropZone
                label="Package archive"
                hint="The name-version.tar.zst file."
                accept=".tar.zst"
                multiple={false}
                disabledReason={blockedReason}
                onFiles={(files) => setArchive(files[0])}
              />
              {archive && (
                <Chosen file={archive} label="archive" onRemove={() => setArchive(undefined)} />
              )}
            </Stack>
            <Stack gap={2}>
              <DropZone
                label="Signature file"
                hint="The matching name-version.sig.toml file."
                accept=".sig.toml"
                multiple={false}
                disabledReason={blockedReason}
                onFiles={(files) => setSignature(files[0])}
              />
              {signature && (
                <Chosen
                  file={signature}
                  label="signature file"
                  onRemove={() => setSignature(undefined)}
                />
              )}
            </Stack>
          </div>
          {mismatch && (
            <InlineMessage tone="warning" title="These two files may not belong together." />
          )}
          <div>
            <Button
              variant="primary"
              icon="upload"
              loading={importer.isPending}
              disabledReason={reason}
              onClick={submit}
            >
              Import package
            </Button>
          </div>
        </Stack>
      </Visible>
      {(result || importer.error) && (
        <Visible category="P0" id="packages.import.result">
          {importer.error ? (
            <InlineMessage tone="danger" title={importer.error.title}>
              {importer.error.detail}
            </InlineMessage>
          ) : (
            result && <ImportResult pkg={result} />
          )}
        </Visible>
      )}
    </Stack>
  );
}
