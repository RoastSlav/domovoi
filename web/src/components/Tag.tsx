import type { ReactNode } from "react";
import type { Tone } from "../design/tone";
import { Tooltip } from "./Tooltip";
import styles from "./status.module.css";

/**
 * A small chip for a label that is not a status: a host's kind, "maintenance",
 * "hub-managed", where a setting comes from. Neutral unless a tone is given. A `hint`
 * explains it on hover and on keyboard focus (the chip then takes a tab stop).
 *
 * Do not use it for a status: that is `StatusPill`, whose tone comes from `tone.ts`.
 */
export function Tag({
  tone,
  hint,
  children,
}: {
  children: ReactNode;
  /** A sentence that explains the label, shown as a tooltip. */
  hint?: string;
  tone?: Tone;
}) {
  const tag = (
    <span className={styles.tag} data-tone={tone} tabIndex={hint ? 0 : undefined}>
      {children}
    </span>
  );
  return hint ? <Tooltip content={hint}>{tag}</Tooltip> : tag;
}
