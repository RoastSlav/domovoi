import type { ResolutionSummary } from "../../api/types";
import { RelativeTime, Stack, Tag, Text, Visible } from "../../components";
import { actorLabel, summaryFieldLabel, summarySourceInfo } from "../../design/knowledgeText";
import { summaryValue } from "./summaryValue";
import styles from "./experience.module.css";

/**
 * Every revision of the summary's fields, newest first, each with who wrote it. Technical
 * detail: collapsed in Standard and Essential.
 */
export function SummaryHistory({
  summary,
  hostNames,
  me,
}: {
  hostNames: ReadonlyMap<string, string>;
  me: string | undefined;
  summary: ResolutionSummary;
}) {
  if (summary.history.length === 0) {
    return null;
  }
  const revisions = [...summary.history].reverse();
  return (
    <Visible
      category="P2"
      id="experience.summary-history"
      label="Edit history"
      count={summary.history.length}
    >
      <Stack gap={2}>
        <ul className={styles.history} aria-label="Revisions">
          {revisions.map((revision) => {
            const source = summarySourceInfo(revision.source);
            return (
              <li key={`${revision.field}:${revision.revision}`} className={styles.historyLine}>
                <Text strong size="sm">
                  {summaryFieldLabel(revision.field)}
                </Text>
                <Tag hint={source.hint}>{source.label}</Tag>
                <Text muted size="sm">
                  Revision {revision.revision}, {actorLabel(revision.actor, me)},{" "}
                  <RelativeTime at={revision.created_at} />
                </Text>
                <Text block size="sm">
                  {summaryValue(revision.field, revision.value, hostNames)}
                </Text>
              </li>
            );
          })}
        </ul>
        {summary.history_truncated && (
          <Text muted size="sm">
            Older revisions are not listed.
          </Text>
        )}
      </Stack>
    </Visible>
  );
}
