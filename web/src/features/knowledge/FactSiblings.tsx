import { Link } from "react-router";
import { useFacts } from "../../api/hooks/knowledge";
import type { KnowledgeFact } from "../../api/types";
import { Button, LoadError, Section, Skeleton, StatusPill, Tag, Visible } from "../../components";
import { needsPassword, sourceInfo } from "../../design/knowledgeText";
import { knowledgeStatusMeta } from "../../design/tone";
import { FailureMessage } from "./FailureMessage";
import { isDisputed, useAcceptAndOpen } from "./FactButtons";
import { factValueText } from "./FactsTable";
import type { SubjectNames } from "./subjects";
import styles from "./factDetail.module.css";

/**
 * The other live values of the same subject and kind. A disputed one can be chosen with
 * "Use this value", which saves it as the owner's own.
 */
export function FactSiblings({ fact, names }: { fact: KnowledgeFact; names: SubjectNames }) {
  const siblings = useFacts({ subject: fact.subject, predicate: fact.predicate });
  const accept = useAcceptAndOpen();
  const others = (siblings.data?.pages ?? [])
    .flatMap((page) => page.items)
    .filter((other) => other.id !== fact.id);
  if (siblings.data !== undefined && others.length === 0) {
    return null;
  }
  return (
    <Visible category="P1" id="fact.siblings">
      <Section title="Other values">
        {siblings.error && siblings.data === undefined ? (
          <LoadError
            error={siblings.error}
            context="load the other values"
            onRetry={siblings.refetch}
          />
        ) : siblings.data === undefined ? (
          <Skeleton shape="rows" count={2} />
        ) : (
          <>
            <FailureMessage error={accept.inline} />
            <ul className={styles.list} aria-label="Other values">
              {others.map((other) => (
                <li key={other.id} className={styles.row}>
                  <Link to={`/knowledge/facts/${encodeURIComponent(other.id)}`}>
                    <span className={styles.value}>{factValueText(other, names)}</span>
                  </Link>
                  <Tag>{sourceInfo(other.source).label}</Tag>
                  <StatusPill size="sm" meta={knowledgeStatusMeta(other.status)} />
                  {isDisputed(other) && (
                    <Button
                      size="sm"
                      variant="primary"
                      hint="Saves it as your own fact. The original stays as history."
                      onClick={() =>
                        accept.start(
                          {
                            id: other.id,
                            expectedVersion: other.version,
                            predicate: other.predicate,
                          },
                          needsPassword("accept", other.predicate),
                        )
                      }
                    >
                      Use this value
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          </>
        )}
        {accept.dialog}
      </Section>
    </Visible>
  );
}
