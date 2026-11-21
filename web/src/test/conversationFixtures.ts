import type { ConversationDetail, ConversationMessage } from "../api/types";
import { T0, nas } from "./fixtures";

/** The ids the live-frame fixture uses. */
export const CONV = "0190a0a0-0000-7000-8000-0000000000c1";
export const TURN = "0190a0a0-0000-7000-8000-0000000000c2";
export const ANSWER = "0190a0a0-0000-7000-8000-0000000000c3";

/** One message, overridable. */
export function message(overrides: Partial<ConversationMessage> = {}): ConversationMessage {
  return {
    id: "m1",
    ordinal: 1,
    role: "user",
    status: "complete",
    turn: TURN,
    text: "Why is the NAS slow?",
    failure: null,
    failure_detail: null,
    tools: [],
    created_at: T0,
    completed_at: T0,
    ...overrides,
  };
}

/** A conversation about `nas` with no messages, overridable. */
export function conversation(overrides: Partial<ConversationDetail> = {}): ConversationDetail {
  return {
    id: CONV,
    title: "",
    scope: { kind: "host", host_id: nas.id, host_name: nas.name },
    operation_id: null,
    message_count: 0,
    created_at: T0,
    updated_at: T0,
    version: 1,
    messages: [],
    turn_in_progress: null,
    as_of_seq: 10,
    ...overrides,
  };
}

/** A conversation about every host. */
export function globalConversation(
  overrides: Partial<ConversationDetail> = {},
): ConversationDetail {
  return conversation({ scope: { kind: "global" }, ...overrides });
}

/** A question and an answer that is still being written. */
export function pendingConversation(): ConversationDetail {
  return conversation({
    title: "Why is the NAS slow?",
    message_count: 2,
    messages: [
      message(),
      message({
        id: ANSWER,
        ordinal: 2,
        role: "assistant",
        status: "pending",
        text: "",
        completed_at: null,
      }),
    ],
    turn_in_progress: { turn: TURN, message: ANSWER },
  });
}

/** A question whose answer failed because the budget is spent. */
export function failedConversation(): ConversationDetail {
  return conversation({
    title: "Why is the NAS slow?",
    message_count: 2,
    messages: [
      message(),
      message({
        id: ANSWER,
        ordinal: 2,
        role: "assistant",
        status: "failed",
        text: "",
        failure: "budget_spent",
        failure_detail: null,
      }),
    ],
  });
}

/** The settings that say a model is set up. */
export const MODEL_SETTINGS = {
  triage: { enabled: true },
  llm_primary: { kind: "anthropic", api_key: "[set]", model: "claude" },
};
