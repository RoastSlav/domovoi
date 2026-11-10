import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Update } from "../types";

/** How often the UI asks whether a newer release is out. */
export const UPDATE_REFRESH_MS = 60 * 60 * 1000;

/** The newest release this hub knows about, refreshed every hour. */
export function useUpdate() {
  return useQuery({
    queryKey: keys.update,
    queryFn: () => call<Update>(api.GET("/api/v1/system/update")),
    refetchInterval: UPDATE_REFRESH_MS,
  });
}

/** Asks GitHub now, at most once a minute; the answer replaces the cached one. */
export function useCheckUpdate() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => call<Update>(api.POST("/api/v1/system/update/check")),
    onSuccess: (update) => client.setQueryData(keys.update, update),
  });
}
