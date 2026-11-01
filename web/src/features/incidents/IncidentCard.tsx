import type { Card } from "../../api/types";
import { Badge } from "../../components/Badge";
import styles from "./incidents.module.css";

/** The incident's card as chat shows it: badge, headline, key facts and the next step. */
export function IncidentCard({ card }: { card: Card }) {
  return (
    <section className={styles.card} data-resolved={card.resolved} aria-label="Summary">
      <Badge value={card.badge} />
      {card.resolved && <span> Resolved</span>}
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
