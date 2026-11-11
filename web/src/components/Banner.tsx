import type { ReactNode } from "react";
import { Icon } from "../design/icons";
import type { Tone } from "../design/tone";
import { IconButton } from "./IconButton";
import { TONE_ICONS } from "./InlineMessage";
import styles from "./message.module.css";

/**
 * A full-width message at the top of the main area, for something that concerns the whole
 * page or the whole hub: live updates lost, an update available, setup unfinished.
 * `solid` fills it and is reserved for one page-level critical state.
 *
 * Do not use it for the result of an action (toast) or a problem with one field or panel
 * (`InlineMessage`). Show at most one at a time.
 */
export function Banner({
  tone,
  title,
  children,
  action,
  onDismiss,
  solid = false,
}: {
  action?: ReactNode;
  children?: ReactNode;
  onDismiss?: () => void;
  solid?: boolean;
  title: string;
  tone: Tone;
}) {
  return (
    <div
      className={styles.banner}
      data-tone={tone}
      data-solid={solid || undefined}
      role={tone === "danger" ? "alert" : "status"}
    >
      <span className={styles.icon}>
        <Icon name={TONE_ICONS[tone]} />
      </span>
      <div className={styles.text}>
        <p className={styles.title}>{title}</p>
        {children && <div className={styles.body}>{children}</div>}
      </div>
      {(action || onDismiss) && (
        <div className={styles.action}>
          {action}
          {onDismiss && <IconButton icon="close" label="Dismiss" size="sm" onClick={onDismiss} />}
        </div>
      )}
    </div>
  );
}
