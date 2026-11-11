import { Icon } from "../design/icons";
import { badgeMeta, type StatusMeta } from "../design/tone";
import { Tooltip } from "./Tooltip";
import styles from "./status.module.css";

/**
 * A status as a pill: a soft tint in its tone, an icon (or a dot) and its label, so colour is
 * never the only signal, and what the status means as a tooltip. `meta` always comes from
 * `design/tone`.
 *
 * Use it for every status in the product. Do not use it for labels that are not a status
 * (that is `Tag`) or for numbers (that is `Count`).
 */
export function StatusPill({
  meta,
  size = "md",
  showIcon = true,
}: {
  meta: StatusMeta;
  /** `false` shows a dot in place of the status's icon, for dense lists. */
  showIcon?: boolean;
  size?: "sm" | "md";
}) {
  return (
    <Tooltip content={meta.hint}>
      <span className={styles.pill} data-tone={meta.tone} data-size={size}>
        <Icon name={showIcon ? meta.icon : "dot"} size={14} />
        {meta.label}
      </span>
    </Tooltip>
  );
}

/** The pill for a chat card's badge: a verdict, a severity or "resolved" in one field. */
export function BadgePill({ value }: { value: string }) {
  return <StatusPill meta={badgeMeta(value)} />;
}
