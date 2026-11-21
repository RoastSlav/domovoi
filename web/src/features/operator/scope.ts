import type { ConversationScope } from "../../api/types";

/** The scope in words, as a tag under a header or beside a row shows it. */
export function scopeLabel(scope: ConversationScope): string {
  return scope.kind === "host" ? `About ${scope.host_name ?? "a removed host"}` : "About all hosts";
}

/** What the composer says the Operator may do, for the conversation's scope. */
export function hintFor(scope: ConversationScope): string {
  return scope.kind === "host" && scope.host_name
    ? `Read-only. It can look at the containers, logs, journal and read skills of ${scope.host_name}. It never changes anything.`
    : "Read-only. It answers from a summary of every host; start a conversation about one host to let it look closer. It never changes anything.";
}
