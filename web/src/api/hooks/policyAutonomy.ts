import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Autonomy } from "../types";

/** The pause switch and the rules whose breaker is open. */
export function useAutonomy() {
  return useQuery({
    queryKey: keys.policyAutonomy,
    queryFn: () => call<Autonomy>(api.GET("/api/v1/policy/autonomy")),
  });
}

function useStore() {
  const client = useQueryClient();
  return (autonomy: Autonomy) => client.setQueryData(keys.policyAutonomy, autonomy);
}

/** Stops every unattended action at once. Needs no password. */
export function usePauseAutonomy() {
  const store = useStore();
  return useMutation({
    mutationFn: ({ reason }: { reason: string }) =>
      call<Autonomy>(api.POST("/api/v1/policy/autonomy/pause", { body: { reason } })),
    onSuccess: store,
  });
}

/** Lets unattended action start again. Needs the password. */
export function useResumeAutonomy() {
  const store = useStore();
  return useMutation({
    mutationFn: ({ password }: { password: string }) =>
      call<Autonomy>(api.POST("/api/v1/policy/autonomy/resume", { body: { password } })),
    onSuccess: store,
  });
}

/** Closes a rule's breaker again. Needs the password. */
export function useResetRule() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ rule, password }: { rule: string; password: string }) =>
      call<Autonomy>(
        api.POST("/api/v1/policy/rules/{rule}/reset", {
          params: { path: { rule } },
          body: { password },
        }),
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.policyAutonomy }),
  });
}
