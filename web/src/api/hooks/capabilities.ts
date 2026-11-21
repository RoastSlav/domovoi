import { useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type Capabilities } from "../types";

/** How often a host's capability inventory refreshes on its own. */
export const CAPABILITIES_REFRESH_MS = 30_000;

/**
 * The capability inventory of host `name`: what the hub would offer there, and why not.
 *
 * A read-only answer: an available row is "the host implements it and the hub enables it",
 * never an approval. Idle while `name` is empty.
 */
export function useCapabilities(name: string) {
  return useQuery(capabilitiesOptions(name));
}

/** The query that reads host `name`'s capability inventory; shared by every reader of it. */
export function capabilitiesOptions(name: string) {
  return {
    queryKey: keys.capabilities(name),
    queryFn: () =>
      call<Capabilities>(
        api.GET("/api/v1/hosts/{name}/capabilities", { params: { path: { name } } }),
      ),
    enabled: name !== "",
    refetchInterval: CAPABILITIES_REFRESH_MS,
  };
}
