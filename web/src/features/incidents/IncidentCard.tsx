import type { Card as CardData } from "../../api/types";
import { BadgePill, Card, Cluster, Stack, Text } from "../../components";
import { badgeMeta, verdictMeta } from "../../design/tone";
import styles from "./incidents.module.css";

/**
 * The incident's summary like the chat card shows it, except a resolved incident wears a
 * Resolved badge and keeps its verdict as secondary text.
 */
export function IncidentCard({ card, verdict }: { card: CardData; verdict?: string | null }) {
  const badge = card.resolved ? "resolved" : card.badge;
  return (
    <Card tone={badgeMeta(badge).tone} label="Summary">
      <Stack gap={2}>
        <Cluster>
          <BadgePill value={badge} />
          {card.resolved && verdict && (
            <Text muted size="sm">
              Triage said {verdictMeta(verdict).label}
            </Text>
          )}
        </Cluster>
        <Text block strong size="lg">
          {card.headline}
        </Text>
        {card.bullets.length > 0 && (
          <ul className={styles.bullets}>
            {card.bullets.map((bullet) => (
              <li key={bullet}>{bullet}</li>
            ))}
          </ul>
        )}
        {card.next_step && (
          <Text block>
            <strong>Next step:</strong> {card.next_step}
          </Text>
        )}
      </Stack>
    </Card>
  );
}
