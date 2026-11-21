import { useMemo, useRef, useState, type ReactNode } from "react";
import {
  useCancelTurn,
  useConversation,
  useRetryMessage,
  useSendMessage,
} from "../../api/hooks/conversations";
import { useHosts } from "../../api/hooks/hosts";
import { useConversationLive } from "../../api/operatorLive";
import type { ConversationDetail } from "../../api/types";
import { scopeLabel } from "./scope";
import {
  InlineMessage,
  LinkButton,
  OperatorThread,
  QueryBoundary,
  Skeleton,
  Tag,
  useActionFeedback,
} from "../../components";
import { useChatOff } from "../chat/useChatOff";
import { clearDraft, readDraft, writeDraft } from "./draft";
import { hintFor } from "./scope";
import { toTurns } from "./turns";
import styles from "./operator.module.css";

function Thread({
  detail,
  actions,
  fill,
}: {
  actions?: ReactNode;
  detail: ConversationDetail;
  fill?: boolean | undefined;
}) {
  const id = detail.id;
  const active = detail.turn_in_progress?.turn;
  const { live } = useConversationLive(id, active);
  const off = useChatOff();
  const hosts = useHosts();
  const [draft, setDraft] = useState(() => readDraft(id));
  const latest = useRef(draft);
  // The composer empties itself on send; that empty value must not erase the saved draft.
  const sending = useRef(false);
  const send = useActionFeedback(useSendMessage(id), {
    success: () => null,
    error: "send the question",
  });
  const retry = useActionFeedback(useRetryMessage(id), {
    success: () => null,
    error: "ask again",
  });
  const cancel = useActionFeedback(useCancelTurn(id), {
    success: () => null,
    error: "stop the answer",
  });
  const turns = useMemo(() => toTurns(detail.messages), [detail.messages]);
  const host = hosts.data?.find((h) => h.id === detail.scope.host_id);
  const offline = detail.scope.kind === "host" && host !== undefined && !host.connected;

  const change = (text: string) => {
    latest.current = text;
    setDraft(text);
    if (!(sending.current && text === "")) {
      writeDraft(id, text);
    }
  };
  const ask = (question: string) => {
    sending.current = true;
    send.mutate(
      { turn: crypto.randomUUID(), text: question },
      {
        onSuccess: () => {
          sending.current = false;
          if (latest.current === "") {
            clearDraft(id);
          }
        },
        onError: () => {
          sending.current = false;
          change(question);
        },
      },
    );
  };
  const shown = active !== undefined && live.turn === active;

  return (
    <div className={styles.thread}>
      <div className={styles.scope}>
        <Tag>{scopeLabel(detail.scope)}</Tag>
        {detail.operation_id && (
          <LinkButton to={`/operations/${detail.operation_id}`} size="sm" variant="ghost">
            Operation
          </LinkButton>
        )}
        {actions && <div className={styles.scopeActions}>{actions}</div>}
      </div>
      {offline && (
        <InlineMessage tone="warning" title={`${host?.name} is offline`}>
          The Operator answers from what the hub last knew.
        </InlineMessage>
      )}
      <OperatorThread
        fill={fill}
        turns={turns}
        label="Ask the Operator"
        hint={hintFor(detail.scope)}
        draft={draft}
        onDraftChange={change}
        onSend={ask}
        sending={send.isPending}
        live={
          shown
            ? { text: live.text, tools: live.tools.map((tool) => ({ name: tool.tool })) }
            : undefined
        }
        onStop={active ? () => cancel.mutate(active) : undefined}
        onRetry={(turn) => retry.mutate(turn.id)}
        offReason={off}
        offAction={
          off ? (
            <LinkButton to="/settings/models" size="sm">
              Set up a model
            </LinkButton>
          ) : undefined
        }
        disabledReason={
          detail.turn_in_progress ? "An answer is being written. Stop it or wait." : undefined
        }
      />
    </div>
  );
}

/**
 * One Operator conversation, loaded and live: the scope, the thread and the composer. The
 * panel and the workspace both show it. Key it by `id` so the draft follows the conversation.
 */
export function OperatorConversation({
  id,
  actions,
  fill,
}: {
  /** Controls for the conversation itself, drawn at the right of the scope line. */
  actions?: ReactNode;
  /** Gives the thread the height of a workspace. */
  fill?: boolean | undefined;
  id: string;
}) {
  const detail = useConversation(id);
  return (
    <QueryBoundary
      query={detail}
      context="load the conversation"
      loading={<Skeleton shape="rows" count={3} />}
    >
      {(data) => <Thread detail={data} actions={actions} fill={fill} />}
    </QueryBoundary>
  );
}
