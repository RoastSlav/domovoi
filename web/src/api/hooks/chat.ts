import { useMutation } from "@tanstack/react-query";
import { api } from "../client";
import { call, type ChatResponse } from "../types";

/** One earlier message of a general chat. */
export interface ChatMessage {
  role: "user" | "assistant";
  text: string;
}

/** A question about the hosts, with the conversation so far; without `host` it is about all. */
export interface ChatInput {
  host?: string | undefined;
  history: ChatMessage[];
  question: string;
}

/** Asks the model about the operator's servers; it can only read. */
export function useChat() {
  return useMutation({
    mutationFn: ({ host, history, question }: ChatInput) =>
      call<ChatResponse>(
        api.POST("/api/v1/chat", {
          body: host ? { host, history, question } : { history, question },
        }),
      ),
  });
}
