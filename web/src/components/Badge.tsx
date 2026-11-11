import { Icon } from "../design/icons";
import { badgeMeta, type StatusMeta } from "../design/tone";
import styles from "./Badge.module.css";

/**
 * A status as a pill: a soft tint in its tone, an icon and its label, so colour is never the
 * only signal, and what it means on hover. Pass a `meta` from `design/tone`, or the `value`
 * of a chat card's badge (a verdict, a severity or "resolved").
 */
export function Badge(props: { meta: StatusMeta } | { value: string }) {
  const meta = "meta" in props ? props.meta : badgeMeta(props.value);
  return (
    <span className={styles.badge} data-tone={meta.tone} title={meta.hint}>
      <Icon name={meta.icon} size={14} />
      {meta.label}
    </span>
  );
}
