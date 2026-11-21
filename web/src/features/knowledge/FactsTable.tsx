import { useMemo } from "react";
import type { KnowledgeFact } from "../../api/types";
import { DataTable, RelativeTime, StatusPill, Tag, Text, type Column } from "../../components";
import { predicateInfo, sourceInfo } from "../../design/knowledgeText";
import { knowledgeStatusMeta } from "../../design/tone";
import { SubjectLink } from "./SubjectLink";
import type { SubjectNames } from "./subjects";
import { valueText } from "./valueText";
import styles from "./knowledge.module.css";

/** The words of a value that was cleared after the retention period. */
export const REMOVED_VALUE = "Removed after the retention period.";

/** The text of a fact's value: its words, or the note that it was removed. */
export function factValueText(fact: KnowledgeFact, names: SubjectNames): string {
  return fact.content_purged ? REMOVED_VALUE : valueText(fact.predicate, fact.value, names);
}

/** The facts as a table of source, status and freshness; each row opens the fact. */
export function FactsTable({
  facts,
  names,
  caption,
}: {
  caption: string;
  facts: readonly KnowledgeFact[];
  names: SubjectNames;
}) {
  const columns = useMemo<Column<KnowledgeFact>[]>(
    () => [
      {
        key: "fact",
        label: "Fact",
        cell: (fact) => <Text strong>{predicateInfo(fact.predicate).label}</Text>,
      },
      {
        key: "about",
        label: "About",
        cell: (fact) => (
          <span className={styles.about}>
            <Text muted>
              <SubjectLink subject={fact.subject} names={names} />
            </Text>
            {predicateInfo(fact.predicate).safety && (
              <Tag hint="Used for safety decisions only when you entered it and it is current.">
                Safety-related
              </Tag>
            )}
          </span>
        ),
      },
      {
        key: "value",
        label: "Value",
        truncate: "clamp2",
        tip: (fact) => factValueText(fact, names),
        cell: (fact) => <span className={styles.value}>{factValueText(fact, names)}</span>,
      },
      {
        key: "source",
        label: "Source",
        cell: (fact) => (
          <Tag hint={sourceInfo(fact.source).hint}>{sourceInfo(fact.source).label}</Tag>
        ),
      },
      {
        key: "status",
        label: "Status",
        cell: (fact) => <StatusPill size="sm" meta={knowledgeStatusMeta(fact.status)} />,
      },
      {
        key: "updated",
        label: "Updated",
        cell: (fact) => <RelativeTime at={fact.last_verified_at ?? fact.observed_at} />,
      },
    ],
    [names],
  );
  return (
    <DataTable
      caption={caption}
      columns={columns}
      rows={facts}
      rowKey={(fact) => fact.id}
      rowHref={(fact) => `/knowledge/facts/${encodeURIComponent(fact.id)}`}
    />
  );
}
