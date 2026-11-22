import { Link } from "react-router";
import { RelativeTime, StatusPill, Tooltip, visibilityAttrs } from "../../components";
import type { AttentionItem } from "./attentionModel";
import styles from "./attention.module.css";

/** "nas", "nas and 2 more", or "a host" when nothing is known. */
export function hostLabel(names: readonly string[]): string {
  const [first, ...rest] = names;
  if (first === undefined) {
    return "a host";
  }
  return rest.length === 0 ? first : `${first} and ${rest.length} more`;
}

/** One thing that waits for the owner: a single link to where it is decided. */
export function AttentionRow({
  item,
  hostNames,
}: {
  item: AttentionItem;
  hostNames: ReadonlyMap<string, string>;
}) {
  const names = item.hostIds.map((id) => hostNames.get(id) ?? id);
  const label = item.kind === "host" ? "" : hostLabel(item.hostIds.length ? names : []);
  return (
    <li className={styles.item} {...visibilityAttrs("P0", "attention.item")}>
      <Link to={item.href} className={styles.row}>
        <StatusPill size="sm" meta={item.meta} />
        <span className={styles.main}>
          <span className={styles.title}>{item.title}</span>
          {item.why !== "" && <span className={styles.why}>{item.why}</span>}
        </span>
        <span className={styles.trail}>
          {label !== "" && (
            <Tooltip content={label}>
              <span className={styles.host}>{label}</span>
            </Tooltip>
          )}
          {item.since !== null && (
            <span className={styles.meta}>
              <RelativeTime at={item.since} />
            </span>
          )}
        </span>
      </Link>
    </li>
  );
}
