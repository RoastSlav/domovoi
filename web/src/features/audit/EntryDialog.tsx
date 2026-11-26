import type { AuditRow } from "../../api/types";
import { Button, CopyBlock, CopyButton, Dialog, KeyValueList } from "../../components";
import styles from "./audit.module.css";

/** One audit entry in full: its position, hash and detail text. */
export function EntryDialog({ row, onClose }: { onClose: () => void; row: AuditRow }) {
  return (
    <Dialog
      title={`Audit entry #${row.seq}`}
      onClose={onClose}
      wide
      actions={<Button onClick={onClose}>Close</Button>}
    >
      <KeyValueList
        mono
        items={[
          { label: "Number", value: `#${row.seq}` },
          { label: "Time", value: row.at },
          { label: "Who", value: row.actor },
          { label: "What", value: row.kind },
          { label: "About", value: row.subject },
          {
            label: "Hash",
            value: row.hash ? (
              <>
                {row.hash} <CopyButton text={row.hash} />
              </>
            ) : (
              "None. This entry has no stored hash."
            ),
          },
        ]}
      />
      {row.detail ? (
        <CopyBlock label="Entry detail" text={row.detail} />
      ) : (
        <p className={styles.muted}>No detail was recorded.</p>
      )}
      <p className={styles.muted}>Secrets are removed before it is shown.</p>
      {row.detail_truncated && <p className={styles.muted}>Shortened to 4 KiB.</p>}
    </Dialog>
  );
}
