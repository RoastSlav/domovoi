import type { IncidentDetail } from "../../api/types";
import { Card, Section, Text } from "../../components";
import styles from "./incidents.module.css";

/** The longest label a "label: value" evidence line may have to be shown as a labelled cell. */
const MAX_LABEL = 32;

interface Cell {
  label: string;
  value: string;
}

/** The evidence as labelled cells, from the report, or from the chat card's "label: value" lines. */
function cellsOf(detail: IncidentDetail): { cells: Cell[]; lines: string[] } {
  const evidence = detail.report?.evidence ?? [];
  if (evidence.length > 0) {
    return { cells: evidence.map(({ label, value }) => ({ label, value })), lines: [] };
  }
  const cells: Cell[] = [];
  const lines: string[] = [];
  for (const bullet of detail.card.bullets) {
    const at = bullet.indexOf(": ");
    const label = at > 0 ? bullet.slice(0, at).trim() : "";
    if (label && label.length <= MAX_LABEL) {
      cells.push({ label, value: bullet.slice(at + 2).trim() });
    } else {
      lines.push(bullet);
    }
  }
  return { cells, lines };
}

/**
 * What was measured, as a dense grid: a small label over its value, three to a row on a
 * wide screen. The hub sends no separate threshold or current value, so this is the
 * evidence the report (or the check itself) gave. Nothing is drawn when there is none.
 */
export function IncidentEvidence({ detail }: { detail: IncidentDetail }) {
  const { cells, lines } = cellsOf(detail);
  if (cells.length === 0 && lines.length === 0) {
    return null;
  }
  return (
    <Section title="Evidence">
      <Card>
        {cells.length > 0 && (
          <dl className={styles.evidence}>
            {cells.map((cell, index) => (
              <div key={`${cell.label}-${index}`} className={styles.evidenceCell}>
                <dt>{cell.label}</dt>
                <dd>{cell.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {lines.length > 0 && (
          <ul className={styles.bullets}>
            {lines.map((line) => (
              <li key={line}>
                <Text size="sm">{line}</Text>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </Section>
  );
}
