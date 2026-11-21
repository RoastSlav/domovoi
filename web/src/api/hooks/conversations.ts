import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type ConversationDetail,
  type ConversationPage,
  type ConversationSummary,
  type ScopeChoice,
  type SentMessage,
} from "../types";

/** The conversation list's filters: a host name and an operation id. */
export interface ConversationFilter {
  host?: string | undefined;
  operation?: string | undefined;
}

/** How many conversations the list and the panel's picker load. */
export const CONVERSATION_PAGE = 50;

/** Conversations, newest change first. */
export function useConversations(filter: ConversationFilter = {}) {
  return useQuery({
    queryKey: keys.conversationList(filter),
    queryFn: () =>
      call<ConversationPage>(
        api.GET("/api/v1/conversations", {
          params: { query: { limit: CONVERSATION_PAGE, ...defined(filter) } },
        }),
      ),
  });
}

/** One conversation with its messages; idle while `id` is undefined. */
export function useConversation(id: string | undefined) {
  return useQuery({
    queryKey: keys.conversation(id ?? ""),
    queryFn: () =>
      call<ConversationDetail>(
        api.GET("/api/v1/conversations/{id}", { params: { path: { id: id ?? "" } } }),
      ),
    enabled: id !== undefined,
  });
}

/** Starts a conversation and caches it, so its page opens without another request. */
export function useCreateConversation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: { scope: ScopeChoice; operation_id?: string | null }) =>
      call<ConversationDetail>(api.POST("/api/v1/conversations", { body })),
    onSuccess: (detail) => {
      client.setQueryData(keys.conversation(detail.id), detail);
      void client.invalidateQueries({ queryKey: keys.conversations });
    },
  });
}

/** Puts a committed pair into the cached thread, so the question and the pending answer show at once. */
function useShowSent(id: string) {
  const client = useQueryClient();
  return (sent: SentMessage) => {
    client.setQueryData<ConversationDetail>(keys.conversation(id), (old) => {
      if (!old) {
        return old;
      }
      const others = old.messages.filter(
        (m) => m.id !== sent.user.id && m.id !== sent.assistant.id,
      );
      const messages = [...others, sent.user, sent.assistant].sort((a, b) => a.ordinal - b.ordinal);
      return {
        ...old,
        messages,
        message_count: messages.length,
        turn_in_progress:
          sent.assistant.status === "pending"
            ? { turn: sent.assistant.turn, message: sent.assistant.id }
            : null,
        as_of_seq: Math.max(old.as_of_seq, sent.as_of_seq),
      };
    });
    void client.invalidateQueries({ queryKey: keys.conversations });
  };
}

/** Asks a question; `turn` is chosen by the caller, so sending it again changes nothing. */
export function useSendMessage(id: string) {
  const show = useShowSent(id);
  return useMutation({
    mutationFn: (body: { turn: string; text: string }) =>
      call<SentMessage>(
        api.POST("/api/v1/conversations/{id}/messages", { params: { path: { id } }, body }),
      ),
    onSuccess: show,
  });
}

/** Asks the last failed or stopped answer again. */
export function useRetryMessage(id: string) {
  const show = useShowSent(id);
  return useMutation({
    mutationFn: (message: string) =>
      call<SentMessage>(
        api.POST("/api/v1/conversations/{id}/messages/{message}/retry", {
          params: { path: { id, message } },
        }),
      ),
    onSuccess: show,
  });
}

/** Stops the answer being written for `turn`. */
export function useCancelTurn(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (turn: string) =>
      call<{ status: string }>(
        api.POST("/api/v1/conversations/{id}/turns/{turn}/cancel", {
          params: { path: { id, turn } },
        }),
      ),
    onSettled: () => {
      void client.invalidateQueries({ queryKey: keys.conversation(id) });
      void client.invalidateQueries({ queryKey: keys.conversations });
    },
  });
}

/** Attaches an operation for read-only context, or detaches it with `null`. */
export function useSetConversationOperation(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (operationId: string | null) =>
      call<ConversationSummary>(
        api.PUT("/api/v1/conversations/{id}/operation", {
          params: { path: { id } },
          body: { operation_id: operationId },
        }),
      ),
    onSuccess: () => void client.invalidateQueries({ queryKey: keys.conversations }),
  });
}

/** Deletes a conversation and its messages. */
export function useDeleteConversation() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) =>
      call(api.DELETE("/api/v1/conversations/{id}", { params: { path: { id } } })),
    onSuccess: (_, id) => {
      client.removeQueries({ queryKey: keys.conversation(id) });
      void client.invalidateQueries({ queryKey: keys.conversations });
    },
  });
}
