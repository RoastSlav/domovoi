import type { ReactNode } from "react";
import { Icon, type IconName } from "../design/icons";
import { describeError } from "../design/messages";
import type { Tone } from "../design/tone";
import { ErrorDetail } from "./ErrorDetail";
import styles from "./message.module.css";

/** The icon that goes with a message of each tone. */
export const TONE_ICONS: Record<Tone, IconName> = {
  danger: "alert-circle",
  warning: "alert",
  success: "check-circle",
  pending: "clock",
  progress: "loader",
  info: "info",
  neutral: "info",
};

/**
 * A toned box inside a form or panel, next to what it is about. A `danger` message
 * interrupts assistive technology (`role="alert"`); the others are announced politely.
 *
 * Use it when the control stays on screen and the person must read or fix something. Do
 * not use it for the result of a quick action (that is a toast) or for something that
 * concerns the whole page (that is a `Banner`).
 */
export function InlineMessage({
  tone,
  title,
  children,
  action,
}: {
  /** What to do about it, usually one small `Button`. */
  action?: ReactNode;
  children?: ReactNode;
  title?: string;
  tone: Tone;
}) {
  return (
    <div className={styles.message} data-tone={tone} role={tone === "danger" ? "alert" : "status"}>
      <span className={styles.icon}>
        <Icon name={TONE_ICONS[tone]} />
      </span>
      <div className={styles.text}>
        {title && <p className={styles.title}>{title}</p>}
        {children && <div className={styles.body}>{children}</div>}
      </div>
      {action && <div className={styles.action}>{action}</div>}
    </div>
  );
}

/**
 * A failed action as an inline message, in the catalogue's words. `context` is the verb
 * phrase of what failed ("save the note"). Renders nothing without an error.
 */
export function InlineError({
  error,
  context,
  action,
}: {
  action?: ReactNode;
  context?: string;
  error: unknown;
}) {
  if (!error) {
    return null;
  }
  const described = describeError(error, context);
  return (
    <InlineMessage tone="danger" title={described.title} action={action}>
      <ErrorDetail error={described} />
    </InlineMessage>
  );
}
