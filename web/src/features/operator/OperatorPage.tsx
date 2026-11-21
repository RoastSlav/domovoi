import { useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { useConversation } from "../../api/hooks/conversations";
import { AllHostsNote } from "../../app/AllHostsNote";
import { usePageName } from "../../app/PageTitle";
import { useMediaQuery, WIDE_QUERY } from "../../app/useMediaQuery";
import { Button, PageHeader, Stack } from "../../components";
import { ConversationList } from "./ConversationList";
import { DeleteConversation } from "./DeleteConversation";
import { NewConversation } from "./NewConversation";
import { OperatorConversation } from "./OperatorConversation";
import { writeActive } from "./active";
import styles from "./operator.module.css";

/** The open conversation with its name in the breadcrumb and a way to delete it. */
function Detail({ id, onDeleted }: { id: string; onDeleted: () => void }) {
  const detail = useConversation(id);
  usePageName(detail.data?.title || (detail.data ? "New conversation" : undefined));
  return (
    <OperatorConversation
      key={id}
      id={id}
      fill
      actions={
        detail.data && (
          <DeleteConversation
            id={id}
            title={detail.data.title}
            pending={detail.data.turn_in_progress != null}
            onDeleted={onDeleted}
          />
        )
      }
    />
  );
}

/**
 * The Operator workspace: every conversation at the left and the open one at the right. On a
 * phone the list and the thread take turns. It lists every host's conversations, so it says so
 * while a host is selected.
 */
export function OperatorPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const wide = useMediaQuery(WIDE_QUERY);
  const operation = params.get("operation") ?? undefined;
  const host = params.get("host") ?? undefined;
  const [creating, setCreating] = useState(operation !== undefined || host !== undefined);
  const open = (next: string) => {
    writeActive(next);
    void navigate(`/operator/${next}`);
  };
  const showList = id === undefined;
  const showRight = wide || id !== undefined || creating;

  return (
    <Stack gap={4}>
      <PageHeader
        title="Operator"
        intro="Ask about your hosts and what happened to them. The Operator reads, never changes anything, and keeps the conversation until you delete it."
        docs="concepts/operator.html"
        actions={
          <Button
            onClick={() => {
              setCreating(true);
              void navigate("/operator");
            }}
          >
            New conversation
          </Button>
        }
      />
      {id === undefined && <AllHostsNote>Conversations are listed for every host.</AllHostsNote>}
      <div className={styles.workspaceHost}>
        <div className={styles.workspace} data-narrow={!wide || id !== undefined}>
          {showList && (
            <div className={wide ? styles.rail : undefined}>
              <ConversationList compact={wide} onNew={() => setCreating(true)} />
            </div>
          )}
          {showRight && (
            <div>
              {id !== undefined ? (
                <Stack gap={3}>
                  <Link to="/operator">Back to conversations</Link>
                  <Detail id={id} onDeleted={() => void navigate("/operator")} />
                </Stack>
              ) : (
                (wide || creating) && (
                  <NewConversation operation={operation} initialHost={host} onCreated={open} />
                )
              )}
            </div>
          )}
        </div>
      </div>
    </Stack>
  );
}
