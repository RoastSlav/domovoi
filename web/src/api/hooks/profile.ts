import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Profile } from "../types";

/** How often a host is polled again while it has no profile yet; the hub's task
 * collects one about every minute, and this picks it up once it exists. */
export const PROFILE_POLL_MS = 10_000;

/** Host `name`'s profile; idle while `name` is blank. The query polls so the
 * collected profile stays fresh, and so the first one appears on its own. */
export function useProfile(name: string) {
  return useQuery({
    queryKey: keys.hostProfile(name),
    queryFn: () =>
      call<Profile>(api.GET("/api/v1/hosts/{name}/profile", { params: { path: { name } } })),
    enabled: name !== "",
    refetchInterval: PROFILE_POLL_MS,
  });
}

/** Asks host `name`'s agent for a new profile now, and shows it once it arrives. */
export function useRefreshProfile(name: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (host: string) =>
      call<Profile>(api.POST("/api/v1/hosts/{name}/profile", { params: { path: { name: host } } })),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.hostProfile(name) }),
  });
}
