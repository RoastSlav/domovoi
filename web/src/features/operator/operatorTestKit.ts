import type { ConversationDetail } from "../../api/types";
import { calls, route } from "../../test/apiMock";
import { ANSWER, MODEL_SETTINGS, conversation, message } from "../../test/conversationFixtures";
import { nas } from "../../test/fixtures";

/** Registers the routes every Operator test needs; `detail` is what the thread loads. */
export function routeOperator(detail: ConversationDetail = conversation()): void {
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/settings", () => MODEL_SETTINGS);
  route("GET /api/v1/conversations/{id}", () => detail);
  route("GET /api/v1/conversations", () => ({
    items: [detail],
    next_cursor: null,
    as_of_seq: detail.as_of_seq,
  }));
}

/** The pair `send_message` returns for `text` asked as `turn`. */
export function sentPair(turn: string, text: string) {
  return {
    user: message({ id: "u2", ordinal: 1, turn, text }),
    assistant: message({
      id: ANSWER,
      ordinal: 2,
      role: "assistant",
      status: "pending",
      turn,
      text: "",
      completed_at: null,
    }),
    as_of_seq: 11,
  };
}

/** The calls made to `key`, in order. */
export function made(key: string) {
  return calls.filter((call) => call.route === key);
}
