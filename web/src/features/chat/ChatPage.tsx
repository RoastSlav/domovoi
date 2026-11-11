import { useState } from "react";
import { useChat, type ChatMessage } from "../../api/hooks/chat";
import { useHosts } from "../../api/hooks/hosts";
import { chatFailureText, ChatThread, MAX_HISTORY_TURNS, PageHeader } from "../../components";
import styles from "./chat.module.css";

/** The stored conversation for `host`; `undefined` is every host. */
function storageKey(host: string | undefined): string {
  return `chat:${host ?? "all"}`;
}

function isMessage(value: unknown): value is ChatMessage {
  return (
    typeof value === "object" &&
    value !== null &&
    "role" in value &&
    "text" in value &&
    (value.role === "user" || value.role === "assistant") &&
    typeof value.text === "string"
  );
}

/** The conversation saved for `host`, or nothing when storage is empty or unavailable. */
function readConversation(host: string | undefined): ChatMessage[] {
  try {
    const saved = sessionStorage.getItem(storageKey(host));
    const parsed: unknown = saved ? JSON.parse(saved) : [];
    return Array.isArray(parsed) ? parsed.filter(isMessage) : [];
  } catch {
    return [];
  }
}

/** Remembers the conversation for `host`; storage may be blocked. */
function saveConversation(host: string | undefined, turns: ChatMessage[]): void {
  try {
    sessionStorage.setItem(storageKey(host), JSON.stringify(turns));
  } catch {
    // Private windows and blocked site data: the conversation lasts for the visit only.
  }
}

/** Forgets the conversation for `host`; storage may be blocked. */
function forgetConversation(host: string | undefined): void {
  try {
    sessionStorage.removeItem(storageKey(host));
  } catch {
    // See saveConversation.
  }
}

/**
 * The newest earlier turns the hub accepts. Only whole question/answer pairs are sent, so
 * a conversation that has outgrown the limit keeps working instead of every request failing.
 */
function recentHistory(turns: ChatMessage[]): ChatMessage[] {
  const start = Math.max(0, turns.length - MAX_HISTORY_TURNS);
  return turns.slice(turns[start]?.role === "assistant" ? start + 1 : start);
}

/**
 * The general chat: one conversation per host, kept in sessionStorage, cleared on demand.
 * With a host chosen the model may look at that host; with "All hosts" it answers from a
 * summary of every host. It only reads, and the hint under the composer says so.
 */
export function ChatPage() {
  const hosts = useHosts();
  const ask = useChat();
  const [host, setHost] = useState<string | undefined>(undefined);
  const [turns, setTurns] = useState<ChatMessage[]>(() => readConversation(undefined));
  const [pending, setPending] = useState("");

  const connected = (hosts.data ?? []).filter((item) => item.connected);
  const offline = (hosts.data ?? []).filter((item) => !item.connected);

  const choose = (name: string | undefined): void => {
    setHost(name);
    setTurns(readConversation(name));
    setPending("");
  };

  const send = async (question: string): Promise<void> => {
    const trimmed = question.trim();
    if (!trimmed || ask.isPending) {
      return;
    }
    setPending(trimmed);
    try {
      const response = await ask.mutateAsync({
        host,
        history: recentHistory(turns),
        question: trimmed,
      });
      const next: ChatMessage[] = [
        ...turns,
        { role: "user", text: trimmed },
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
        intro="Ask about your servers in general. It can read what the agents report, and it never changes anything."
      />
      <div className={styles.host}>
        <label htmlFor="chat-host">Host</label>
        <select
          id="chat-host"
          value={host ?? ""}
          onChange={(event) => choose(event.target.value || undefined)}
          disabled={ask.isPending}
        >
          <option value="">All hosts</option>
          {[...connected, ...offline].map((item) => (
            <option key={item.id} value={item.name}>
              {item.connected ? item.name : `${item.name} (offline: no tools)`}
            </option>
          ))}
        </select>
      </div>
      <ChatThread
        turns={turns}
        pending={pending}
        error={ask.error ? chatFailureText(ask.error) : undefined}
        hint="Read-only. With a host chosen, it can look at that host's containers, logs, journal and read skills."
        label="Ask about your servers"
        sending={ask.isPending}
        onSend={send}
        onClear={clear}
      />
    </>
  );
}
