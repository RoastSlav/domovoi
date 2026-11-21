import { NavLink } from "react-router";
import { useConversations } from "../../api/hooks/conversations";
import type { ConversationSummary } from "../../api/types";
import { Button, EmptyState, QueryBoundary, RelativeTime, Skeleton } from "../../components";
import { scopeLabel } from "./scope";
import styles from "./operator.module.css";

/** One conversation as a row; the whole row is the link. */
function Row({ conversation }: { conversation: ConversationSummary }) {
  return (
    <li>
      <NavLink to={`/operator/${conversation.id}`} className={styles.row ?? ""}>
        <span className={styles.rowTitle}>{conversation.title || "New conversation"}</span>
        <span className={styles.rowMeta}>
          <span>{scopeLabel(conversation.scope)}</span>
          <RelativeTime at={conversation.updated_at} />
        </span>
      </NavLink>
    </li>
  );
}

/** Every conversation, newest change first, with loading, empty and error states. */
export function ConversationList({
  host,
  operation,
  onNew,
}: {
  host?: string | undefined;
  onNew: () => void;
  operation?: string | undefined;
}) {
  const list = useConversations({ host, operation });
  return (
    <QueryBoundary
      query={list}
      context="load the conversations"
      loading={<Skeleton shape="rows" count={4} />}
      isEmpty={(page) => page.items.length === 0}
      empty={
        <EmptyState
          icon="chat"
          title="No conversations yet"
          action={<Button onClick={onNew}>Start a conversation</Button>}
        >
          <p>Ask the Operator about a host or about every host. It only reads.</p>
        </EmptyState>
      }
    >
      {(page) => (
        <ul className={styles.list} aria-label="Conversations">
          {page.items.map((conversation) => (
            <Row key={conversation.id} conversation={conversation} />
          ))}
        </ul>
      )}
    </QueryBoundary>
  );
}
