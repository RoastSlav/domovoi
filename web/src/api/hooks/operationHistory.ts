import { useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type OperationHistory } from "../types";

/**
 * How an operation unfolded, for replaying its graph. Idle until `enabled` (the replay bar is
 * open) and an id exist. It sits under the operation's own key, so a journal frame for the
 * operation refreshes it with the page.
 */
export function useOperationHistory(id: string | undefined, enabled: boolean) {
  return useQuery({
    queryKey: keys.operationHistory(id ?? ""),
    enabled: enabled && Boolean(id),
    queryFn: () =>
      call<OperationHistory>(
        api.GET("/api/v1/operations/{id}/history", { params: { path: { id: id ?? "" } } }),
      ),
  });
}
