import { useQueries } from "@tanstack/react-query";
import { api } from "../../api/client";
import { keys } from "../../api/queryClient";
import { call, type ContainmentStatus, type Host } from "../../api/types";
import type { ContainmentAttentionInput } from "./containmentItems";

/** What the attention list needs to know about the containment source. */
export interface ContainmentSource {
  error: unknown;
  isError: boolean;
  isPending: boolean;
  /** Undefined until the hosts are known and at least one status answered. */
  rows: ContainmentAttentionInput[] | undefined;
  retry: () => void;
}

/**
 * Each host's containment status, for the attention list. A host without an enrolled agent has no
 * helper and is left out. A status that failed to load makes the source an error only when
 * none loaded, so one broken host never hides the others, and an empty answer is only what
 * the hub said.
 */
export function useContainmentSources(hosts: readonly Host[] | undefined): ContainmentSource {
  const named = (hosts ?? []).filter((host) => host.agent_enrolled);
  const results = useQueries({
    queries: named.map((host) => ({
      queryKey: keys.hostContainment(host.name),
      queryFn: () =>
        call<ContainmentStatus>(
          api.GET("/api/v1/hosts/{name}/containment", { params: { path: { name: host.name } } }),
        ),
    })),
  });
  const loaded = results.flatMap((result, index) => {
    const host = named[index];
    return result.data && host
      ? [{ hostId: host.id, hostName: host.name, status: result.data }]
      : [];
  });
  const failed = results.filter((result) => result.isError);
  const settled = results.every((result) => !result.isPending);
  const nothing = named.length > 0 && loaded.length === 0;
  return {
    error: failed[0]?.error,
    isError: nothing && failed.length > 0,
    isPending: hosts === undefined || (nothing && !settled),
    rows: hosts === undefined || nothing ? undefined : loaded,
    retry: () => {
      for (const result of failed) {
        void result.refetch();
      }
    },
  };
}
