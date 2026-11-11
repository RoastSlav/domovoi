import { useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, defined, type Overview } from "../types";

/** How often the navigation's counts refresh on their own; live events refresh them sooner. */
export const OVERVIEW_REFRESH_MS = 30_000;

/**
 * What needs the operator, as counts for the navigation: for `host` (by name), or for every
 * host when it is absent. Polling stops while the tab is hidden.
 */
export function useOverview(host: string | undefined) {
  return useQuery({
    queryKey: keys.overviewFor(host),
    queryFn: () =>
      call<Overview>(api.GET("/api/v1/overview", { params: { query: defined({ host }) } })),
    refetchInterval: OVERVIEW_REFRESH_MS,
    refetchIntervalInBackground: false,
  });
}
