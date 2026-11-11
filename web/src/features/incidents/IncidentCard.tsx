import type { Card } from "../../api/types";
import { BadgePill } from "../../components";
import { verdictMeta } from "../../design/tone";
import styles from "./incidents.module.css";

/**
 * The incident's card like the chat card shows it, except a resolved incident wears a
 * Resolved badge and keeps its verdict as secondary text.
 */
export function IncidentCard({ card, verdict }: { card: Card; verdict?: string | null }) {
  return (
    <section className={styles.card} data-resolved={card.resolved} aria-label="Summary">
      <BadgePill value={card.resolved ? "resolved" : card.badge} />
      {card.resolved && verdict && (
        <span className={styles.meta}> Triage said {verdictMeta(verdict).label}</span>
      )}
      <h1>{card.headline}</h1>
      {card.bullets.length > 0 && (
        <ul>
          {card.bullets.map((bullet) => (
            <li key={bullet}>{bullet}</li>
          ))}
        </ul>
      )}
      {card.next_step && (
        <p className={styles.nextStep}>
          <strong>Next step:</strong> {card.next_step}
        </p>
      )}
    </section>
  );
}
