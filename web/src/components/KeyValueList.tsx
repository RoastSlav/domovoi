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
 * Facts about one thing as a definition list: label on the left, value on the right,
 * stacked on a phone.
 *
 * Use it for the details of a single object (a host, a skill). Many objects with the same
 * fields are a `DataTable`.
 */
export function KeyValueList({ items }: { items: readonly KeyValue[] }) {
  return (
    <dl className={styles.kv}>
      {items.map((item) => (
        <div key={item.label} className="contents">
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
