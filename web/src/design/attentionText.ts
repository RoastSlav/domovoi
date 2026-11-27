import { hostConnectionMeta } from "./tone";

/** What the owner is told to do about an attention item, one plain sentence ("" when the title says it all). */
export type AttentionWhy =
  | { kind: "result_unknown_operation" }
  | { kind: "question"; expiresAt?: string | undefined }
  | { kind: "paused"; reason: string | undefined }
  | { kind: "host"; connection: string };

/** The sentence for `why`, built from the existing vocabularies where they already say it. */
export function attentionWhy(why: AttentionWhy): string {
  switch (why.kind) {
    case "result_unknown_operation":
      return "It may have run on the host. Check the host before continuing.";
    case "question": {
      const until = why.expiresAt ? new Date(why.expiresAt) : undefined;
      const at =
        until && !Number.isNaN(until.getTime())
          ? until.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
          : undefined;
      return at
        ? `Triage asked this and is waiting for your answer. It carries on at ${at} without you.`
        : "Triage asked this and is waiting for your answer.";
    }
    case "host":
      return hostConnectionMeta(why.connection).hint ?? "";
    case "paused":
      switch (why.reason) {
        case "budget":
          return "A limit is spent. Raise it to continue, or cancel it.";
        case "error":
          return "It paused after an error.";
        case "manual":
          return "It was paused by a person.";
        default:
          return "It is paused.";
      }
  }
}
