import type { ReactNode } from "react";
import type { Tone } from "../design/tone";
import styles from "./status.module.css";

/**
 * A small chip for a label that is not a status: a host's kind, "maintenance",
 * "hub-managed", where a setting comes from. Neutral unless a tone is given.
 *
 * Do not use it for a status: that is `StatusPill`, whose tone comes from `tone.ts`.
 */
export function Tag({ tone, children }: { children: ReactNode; tone?: Tone }) {
  return (
    <span className={styles.tag} data-tone={tone}>
      {children}
    </span>
  );
}
