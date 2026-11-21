import { Link } from "react-router";
import { useFacts } from "../../api/hooks/knowledge";
import type { KnowledgeFact } from "../../api/types";
import { LoadError, RelativeTime, Skeleton, Tag, Text } from "../../components";
import { sourceInfo } from "../../design/knowledgeText";
import { factValueText } from "./FactsTable";
import type { SubjectNames } from "./subjects";
import styles from "./factDetail.module.css";

/** The earlier, replaced values of the same subject and kind, newest first. */
export function FactHistory({ fact, names }: { fact: KnowledgeFact; names: SubjectNames }) {
  const history = useFacts({
    subject: fact.subject,
    predicate: fact.predicate,
    status: "superseded",
  });
  if (history.error && history.data === undefined) {
    return <LoadError error={history.error} context="load the history" onRetry={history.refetch} />;
  }
  if (history.data === undefined) {
    return <Skeleton shape="rows" count={2} />;
  }
  const rows = history.data.pages.flatMap((page) => page.items);
  if (rows.length === 0) {
    return <Text muted>No earlier values.</Text>;
  }
  return (
    <ul className={styles.list} aria-label="Earlier values">
      {rows.map((row) => (
        <li key={row.id} className={styles.row}>
          <Link to={`/knowledge/facts/${encodeURIComponent(row.id)}`}>
            <span className={styles.value}>{factValueText(row, names)}</span>
          </Link>
          <Tag>{sourceInfo(row.source).label}</Tag>
          <Text muted>
            <RelativeTime at={row.observed_at} />
          </Text>
        </li>
      ))}
    </ul>
  );
}
