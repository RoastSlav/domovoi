import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  type AckRule,
  type ExpectedSignal,
  type AckRuleInput,
  type AckRulePatch,
} from "../types";

/** Every rule, expired ones included; soonest expiry first, rules that never expire last. */
export function useAckRules(enabled = true) {
  return useQuery({
    enabled,
    queryKey: keys.ackRules,
    queryFn: () => call<AckRule[]>(api.GET("/api/v1/ack-rules")),
  });
}

/** Events known conditions explained, newest first, for one host or every host. */
export function useExpectedSignals(host?: string, enabled = true) {
  return useQuery({
    enabled,
    queryKey: keys.expectedSignals(host),
    queryFn: () =>
      call<ExpectedSignal[]>(
        api.GET("/api/v1/expected-signals", {
          params: { query: { ...(host ? { host } : {}), limit: 500 } },
        }),
      ),
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

/** Edits a rule: its host, target, note, conditions or expiry. */
export function useUpdateAckRule() {
  const onSuccess = useRefreshRules();
  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: AckRulePatch }) =>
      call<AckRule>(api.PATCH("/api/v1/ack-rules/{id}", { params: { path: { id } }, body: patch })),
    onSuccess,
  });
}
