import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, defined, type ActionPage } from "../types";

/** Actions with `status` (all when absent) on `host` (by name; every host when absent), newest first. */
export function useActions(status?: string, host?: string) {
  return useQuery({
    queryKey: keys.actionList(status, host),
    queryFn: () =>
      call<ActionPage>(
        api.GET("/api/v1/actions", { params: { query: defined({ status, host, limit: 200 }) } }),
      ),
  });
}

function useActionChange<T>(change: (input: T) => Promise<unknown>) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: change,
    onSuccess: async () => {
      await Promise.all([
        client.invalidateQueries({ queryKey: keys.actions }),
        // An incident's page lists its actions; the incident lists do not.
        client.invalidateQueries({ queryKey: keys.incidentDetails }),
        client.invalidateQueries({ queryKey: keys.overview }),
      ]);
    },
  });
}

/** What approving an action sends. */
export interface ApproveInput {
  id: string;
  /** For destructive skills: the skill's name, typed by the user. */
  confirmText?: string;
}

/** Approves an action as the logged-in user. */
export function useApprove() {
  return useActionChange((input: ApproveInput) =>
    call(
      api.POST("/api/v1/actions/{id}/approve", {
        params: { path: { id: input.id } },
        body: input.confirmText === undefined ? {} : { confirm_text: input.confirmText },
      }),
    ),
  );
}

/** Denies an action. */
export function useDeny() {
  return useActionChange((id: string) =>
    call(api.POST("/api/v1/actions/{id}/deny", { params: { path: { id } } })),
  );
}
