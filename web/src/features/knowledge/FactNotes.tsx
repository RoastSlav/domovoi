import { useSession } from "../../api/hooks/session";
import type { KnowledgeFact } from "../../api/types";
import { InlineMessage, LinkButton, RelativeTime, Visible } from "../../components";
import { actorLabel } from "../../design/knowledgeText";
import { factActions } from "./factActions";
import { parseSubject, type SubjectNames } from "./subjects";
import styles from "./factDetail.module.css";

/**
 * What the owner must know before acting: a retracted, purged or replaced fact, and that a
 * management path is confirmed on its own page. Draws nothing when none applies.
 */
export function FactNotes({ fact, names }: { fact: KnowledgeFact; names: SubjectNames }) {
  const session = useSession();
  const notes = [];
  if (fact.status === "retracted") {
    notes.push(
      <InlineMessage key="retracted" tone="neutral">
        Retracted{" "}
        {fact.retracted_at ? (
          <>
            <RelativeTime at={fact.retracted_at} />{" "}
          </>
        ) : null}
        by {actorLabel(fact.actor, session.data?.user.id)}.
      </InlineMessage>,
    );
  }
  if (fact.content_purged) {
    notes.push(
      <InlineMessage key="purged" tone="neutral">
        The content was removed after the retention period. Only this record remains.
      </InlineMessage>,
    );
  }
  if (fact.status === "superseded") {
    notes.push(
      <InlineMessage key="superseded" tone="neutral">
        A newer value replaced this one. See History.
      </InlineMessage>,
    );
  }
  if (factActions(fact).confirmElsewhere) {
    const parsed = parseSubject(fact.subject);
    const host = parsed?.kind === "host" && parsed.host ? names.hosts.get(parsed.host) : undefined;
    notes.push(
      <InlineMessage
        key="confirm"
        tone="info"
        action={
          host ? (
            <LinkButton to={`/knowledge/paths?host=${encodeURIComponent(host)}`}>
              Open management paths
            </LinkButton>
          ) : undefined
        }
      >
        Confirm this path on the Management paths page.
      </InlineMessage>,
    );
  }
  if (notes.length === 0) {
    return null;
  }
  return (
    <Visible category="P0" id="fact.notes">
      <div className={styles.notes}>{notes}</div>
    </Visible>
  );
}
