import type { ImportResult as Result } from "../../api/types";
import { Cluster, Stack, Tag, Text } from "../../components";
import styles from "./notes.module.css";

/**
 * What an import did, as counts: notes imported in green, files skipped in grey, each with
 * the hub's reason. The hub reports every file it left out as skipped, so there is no
 * separate "failed" count; a request that fails as a whole is shown by the dialog.
 */
export function ImportResult({ result }: { result: Result }) {
  return (
    <Stack gap={2}>
      <div role="status">
        <Cluster gap={2}>
          <Tag tone="success">
            {result.created === 1 ? "1 note imported" : `${result.created} notes imported`}
          </Tag>
          <Tag tone="neutral">
            {result.skipped === 1 ? "1 file skipped" : `${result.skipped} files skipped`}
          </Tag>
        </Cluster>
      </div>
      {result.skipped_files.length > 0 && (
        <ul className={styles.listOfFiles} aria-label="Skipped files">
          {result.skipped_files.map((file) => (
            <li key={file.name}>
              <Text mono>{file.name}</Text>
              <Text muted>{`: ${file.reason}`}</Text>
            </li>
          ))}
        </ul>
      )}
    </Stack>
  );
}
