import type { ReactNode } from "react";
import { InfoTip } from "./InfoTip";
import styles from "./layout.module.css";

/** One row of a key and value list. */
export interface KeyValue {
  /** A longer explanation of the label, behind an info button. */
  hint?: string;
  label: string;
  value: ReactNode;
}

/**
 * Facts about one thing as a definition list: a muted label and its value, in two columns
 * when the container is wide enough and stacked on a phone.
 *
 * Use it for the details of a single object (a host, a skill). Many objects with the same
 * fields are a `DataTable`.
 */
export function KeyValueList({
  items,
  mono = false,
}: {
  items: readonly KeyValue[];
  /** Values in the monospace font, wrapped anywhere: for arguments, paths and IDs. */
  mono?: boolean;
}) {
  return (
    <dl className={styles.kv} data-mono={mono || undefined}>
      {items.map((item) => (
        <div key={item.label} className={styles.kvRow}>
          <dt>
            {item.label}
            {item.hint && <InfoTip title={item.label}>{item.hint}</InfoTip>}
          </dt>
          <dd>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}
