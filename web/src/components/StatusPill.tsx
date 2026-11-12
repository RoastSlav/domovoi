import { badgeMeta, type StatusMeta } from "../design/tone";
import { Tooltip } from "./Tooltip";
import styles from "./status.module.css";

/**
 * A status as a pill: a low tint in its tone, a 6 px dot in the tone's colour and its label, so
 * colour is never the only signal, and what the status means as a tooltip. `meta` always
 * comes from `design/tone`. `sm` is 18 px high, for dense lists.
 *
 * Use it for every status in the product. Do not use it for labels that are not a status
 * (that is `Tag`) or for numbers (that is `Count`).
 */
export function StatusPill({ meta, size = "md" }: { meta: StatusMeta; size?: "sm" | "md" }) {
  return (
    <Tooltip content={meta.hint}>
      <span className={styles.pill} data-tone={meta.tone} data-size={size}>
        <span className={styles.dot} aria-hidden="true" />
        {meta.label}
      </span>
    </Tooltip>
  );
}

/** The pill for a chat card's badge: a verdict, a severity or "resolved" in one field. */
export function BadgePill({ value }: { value: string }) {
  return <StatusPill meta={badgeMeta(value)} />;
}
