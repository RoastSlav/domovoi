import { useRef, useState } from "react";
import { useChat, type ChatMessage } from "../../api/hooks/chat";
import { useHosts } from "../../api/hooks/hosts";
import { useSettings } from "../../api/hooks/system";
import { useSelectedHost } from "../../app/HostContext";
import {
  ChatThread,
  EmptyState,
  InlineMessage,
  LinkButton,
  PageHeader,
  Stack,
  Tag,
} from "../../components";
import styles from "./chat.module.css";
import {
  forgetConversation,
  readConversation,
  recentHistory,
  saveConversation,
  starters,
} from "./conversation";
import { useChatOff } from "./useChatOff";

/**
 * The general chat: one conversation per host, kept in sessionStorage, cleared on demand.
 * It is about the host selected for the whole app (the header says which); with a host
 * chosen the model may look at that host, with "All hosts" it answers from a summary of
 * every host. It only reads, and the hint under the composer says so.
 */
export function ChatPage() {
  const [host] = useSelectedHost();
  // Each host has its own conversation: a new host starts from what was saved for it.
  return <Conversation key={host ?? ""} host={host} />;
}

function Conversation({ host }: { host: string | undefined }) {
  const hosts = useHosts();
  const ask = useChat();
  const off = useChatOff();
  const settings = useSettings();
  // Without a model there is nothing to ask: say so once instead of showing a box that fails.
  const noModel =
    settings.data !== undefined &&
    (!settings.data.triage.enabled || settings.data.llm_primary === null);
  const [turns, setTurns] = useState<ChatMessage[]>(() => readConversation(host));
  const [pending, setPending] = useState("");
  const request = useRef<AbortController | null>(null);
  const offline = host !== undefined && hosts.data?.some((h) => h.name === host && !h.connected);

  const send = async (question: string): Promise<void> => {
    const controller = new AbortController();
    request.current = controller;
    setPending(question);
    try {
      const response = await ask.mutateAsync({
        host,
        history: recentHistory(turns),
        question,
        signal: controller.signal,
      });
      const next: ChatMessage[] = [
        ...turns,
        { role: "user", text: question },
        { role: "assistant", text: response.answer },
      ];
      setTurns(next);
      saveConversation(host, next);
    } finally {
      setPending("");
    }
  };

  const clear = (): void => {
    setTurns([]);
    setPending("");
    forgetConversation(host);
  };

  return (
    <>
      <PageHeader
        title="Chat"
        docs="concepts/how-it-works.html"
        intro="Ask about your servers; it reads what the agents report and never changes anything."
        meta={<Tag tone="info">{host ? `About ${host}` : "About all hosts"}</Tag>}
      />
      <Stack gap={4}>
        {offline && (
          <InlineMessage tone="warning" title={`${host} is offline`}>
            The model cannot look at it now. It answers from what the hub last knew.
          </InlineMessage>
        )}
        {noModel ? (
          <EmptyState
            icon="chat"
            title="Set up a model to ask about your servers"
            action={
              <LinkButton to="/settings/models" variant="primary">
                Open Models in Settings
              </LinkButton>
            }
          />
        ) : (
          <div className={styles.thread}>
            <ChatThread
              turns={turns}
              pending={pending}
              hint={
                host
                  ? `Read-only. It can look at the containers, logs, journal and read skills of ${host}.`
                  : "Read-only. It answers from a summary of every host; choose a host in the top bar to let it look closer."
              }
              label="Ask about your servers"
              disabledReason={off}
              offAction={
                <LinkButton to="/settings/models" size="sm">
                  Open model settings
                </LinkButton>
              }
              sending={ask.isPending}
              starters={starters(host)}
              onSend={send}
              onStop={() => request.current?.abort()}
              onClear={clear}
            />
          </div>
        )}
      </Stack>
    </>
  );
}
