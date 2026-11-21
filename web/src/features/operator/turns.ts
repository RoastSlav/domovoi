import type { ConversationMessage } from "../../api/types";
import type { OperatorTurn } from "../../components";
import { describeTurnFailure } from "../../design/messages";

/**
 * The hub's messages as the thread shows them. Only the last message can be retried, and
 * only when it is an answer that failed or was stopped.
 */
export function toTurns(messages: readonly ConversationMessage[]): OperatorTurn[] {
  return messages.map((message, index) => {
    const status = (["pending", "failed", "cancelled"] as const).find((s) => s === message.status);
    const settled = status === "failed" || status === "cancelled";
    const assistant = message.role === "assistant";
    return {
      id: message.id,
      at: message.completed_at ?? message.created_at,
      role: assistant ? "assistant" : "user",
      status: status ?? "complete",
      text: message.text,
      tools: message.tools.map((tool) => ({ name: tool.tool })),
      ...(assistant && settled
        ? { problem: describeTurnFailure(message.failure, message.failure_detail) }
        : {}),
      ...(assistant && settled && index === messages.length - 1 ? { canRetry: true } : {}),
    };
  });
}
