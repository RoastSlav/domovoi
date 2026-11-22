import type { AttentionItem } from "./attentionModel";
import { AttentionRow } from "./AttentionRow";
import styles from "./attention.module.css";

/** How many items the Control Room shows before pointing at the attention centre. */
export const PREVIEW_ROWS = 5;

/** Attention items as one bordered list of link rows. */
export function AttentionRows({
  items,
  hostNames,
}: {
  items: readonly AttentionItem[];
  hostNames: ReadonlyMap<string, string>;
}) {
  return (
    <ol className={styles.list}>
      {items.map((item) => (
        <AttentionRow key={item.key} item={item} hostNames={hostNames} />
      ))}
    </ol>
  );
}
