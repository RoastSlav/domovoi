import type { ReactNode } from "react";
import { Link } from "react-router";
import type { Tone } from "../design/tone";
import { toneStripe } from "./ToneStripe";
import styles from "./layout.module.css";

/**
 * A raised surface that groups related content. `tone` gives it a left stripe in that tone;
 * `to` makes the whole card a link.
 *
 * Use it for one thing among several (a host, an action, a note). Do not put cards inside
 * cards, and do not tint a card: the stripe and a `StatusPill` carry the status.
 */
export function Card({
  tone,
  to,
  label,
  children,
}: {
  children: ReactNode;
  /** Names the card for assistive technology when its content does not. */
  label?: string;
  /** Where the card leads; it then renders as a link. */
  to?: string;
  tone?: Tone | undefined;
}) {
  if (to) {
    return (
      <Link
        to={to}
        className={styles.card}
        aria-label={label}
        data-interactive
        {...toneStripe(tone)}
      >
        {children}
      </Link>
    );
  }
  return (
    <div
      className={styles.card}
      {...(label ? { role: "group", "aria-label": label } : {})}
      {...toneStripe(tone)}
    >
      {children}
    </div>
  );
}
