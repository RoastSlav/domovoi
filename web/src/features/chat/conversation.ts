import type { ChatMessage } from "../../api/hooks/chat";
import { MAX_HISTORY_TURNS } from "../../components";

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
export function readConversation(host: string | undefined): ChatMessage[] {
  try {
    const saved = sessionStorage.getItem(storageKey(host));
    const parsed: unknown = saved ? JSON.parse(saved) : [];
    return Array.isArray(parsed) ? parsed.filter(isMessage) : [];
  } catch {
    return [];
  }
}

/** Remembers the conversation for `host`; storage may be blocked. */
export function saveConversation(host: string | undefined, turns: ChatMessage[]): void {
  try {
    sessionStorage.setItem(storageKey(host), JSON.stringify(turns));
  } catch {
    // Private windows and blocked site data: the conversation lasts for the visit only.
  }
}

/** Forgets the conversation for `host`; storage may be blocked. */
export function forgetConversation(host: string | undefined): void {
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
export function recentHistory(turns: ChatMessage[]): ChatMessage[] {
  const start = Math.max(0, turns.length - MAX_HISTORY_TURNS);
  return turns.slice(turns[start]?.role === "assistant" ? start + 1 : start);
}

/** A few short questions to start from, about `host` or about every host. */
export function starters(host: string | undefined): string[] {
  return host
    ? [
        `Is anything wrong on ${host} right now?`,
        `What changed on ${host} in the last day?`,
        `Which containers on ${host} restarted recently?`,
        `How full are the disks on ${host}?`,
      ]
    : [
        "Is anything wrong right now?",
        "Which host needs attention first?",
        "What happened overnight?",
      ];
}
