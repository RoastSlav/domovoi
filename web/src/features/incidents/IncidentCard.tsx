import type { Card } from "../../api/types";
import { Badge, badgeLabel } from "../../components/Badge";
import styles from "./incidents.module.css";

/** The incident's card as chat shows it: badge, headline, key facts and the next step. */
export function IncidentCard({ card }: { card: Card }) {
  return (
    <section className={styles.card} data-resolved={card.resolved} aria-label="Summary">
      <Badge value={card.resolved ? "resolved" : card.badge} />
      {card.resolved && <span className={styles.meta}>Triage said {badgeLabel(card.badge)}</span>}
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
