import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type AckRule, type AckRuleInput } from "../types";

/** Every rule, expired ones included; soonest expiry first, rules that never expire last. */
export function useAckRules() {
  return useQuery({
    queryKey: keys.ackRules,
    queryFn: () => call<AckRule[]>(api.GET("/api/v1/ack-rules")),
  });
}

function useRefreshRules() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: keys.ackRules });
}

/** Creates a rule. */
export function useCreateAckRule() {
  const onSuccess = useRefreshRules();
  return useMutation({
    mutationFn: (body: AckRuleInput) => call<AckRule>(api.POST("/api/v1/ack-rules", { body })),
    onSuccess,
  });
}

/** Deletes a rule. */
export function useDeleteAckRule() {
  const onSuccess = useRefreshRules();
  return useMutation({
    mutationFn: (id: string) =>
      call(api.DELETE("/api/v1/ack-rules/{id}", { params: { path: { id } } })),
    onSuccess,
  });
}
