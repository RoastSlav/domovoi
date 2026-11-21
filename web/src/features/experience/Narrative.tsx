import type { SummaryField } from "../../api/types";
import { Tag, Text } from "../../components";
import { NARRATIVE_FIELDS, summaryFieldLabel, summarySourceInfo } from "../../design/knowledgeText";
import { summaryValue } from "./summaryValue";
import styles from "./experience.module.css";

const NO_HOSTS: ReadonlyMap<string, string> = new Map();

/**
 * The problem, cause and fix of a past operation as labelled lines, each with the source of
 * its text. Model-drafted text is never shown without its source, and is always plain text.
 * `compact` cuts each line to three.
 */
export function Narrative({
  fields,
  compact = false,
}: {
  compact?: boolean;
  fields: readonly SummaryField[];
}) {
  const lines = NARRATIVE_FIELDS.flatMap((name) => fields.filter((field) => field.field === name));
  if (lines.length === 0) {
    return null;
  }
  return (
    <dl className={styles.narrative}>
      {lines.map((field) => {
        const source = summarySourceInfo(field.source);
        return (
          <div key={field.field} className={styles.narrativeLine}>
            <dt>
              <Text strong size="sm">
                {summaryFieldLabel(field.field)}
              </Text>
            </dt>
            <dd className={styles.narrativeValue}>
              <span className={compact ? styles.clamp : undefined}>
                {summaryValue(field.field, field.value, NO_HOSTS)}
              </span>
              <Tag hint={source.hint}>{source.label}</Tag>
            </dd>
          </div>
        );
      })}
    </dl>
  );
}
