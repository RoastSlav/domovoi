import { actionStatusMeta, hostConnectionMeta } from "./tone";

/** What the owner is told to do about an attention item, one plain sentence ("" when the title says it all). */
export type AttentionWhy =
  | { kind: "result_unknown_operation" }
  | { kind: "result_unknown_action" }
  | { kind: "question" }
  | { kind: "paused"; reason: string | undefined }
  | { kind: "host"; connection: string };

/** The sentence for `why`, built from the existing vocabularies where they already say it. */
export function attentionWhy(why: AttentionWhy): string {
  switch (why.kind) {
    case "result_unknown_operation":
      return "It may have run on the host. Check the host before continuing.";
    case "result_unknown_action":
      return actionStatusMeta("outcome_unknown").hint ?? "";
    case "question":
      return "";
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
