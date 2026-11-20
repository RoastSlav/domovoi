import { useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, defined, type ActionPage } from "../types";

/** Which actions a list shows. */
export interface ActionFilters {
  /** `true`: no longer waiting for approval; `false`: still waiting. */
  decided?: boolean | undefined;
  /** Only this host, by name. */
  host?: string | undefined;
  /** Only actions that changed at or after this time, RFC 3339. */
  since?: string | undefined;
  /** Only this status name. */
  status?: string | undefined;
}

/** Actions matching `filters` (all of them when empty), newest first. */
export function useActions(filters: ActionFilters = {}, enabled = true) {
  const query = defined({ ...filters, limit: 200 });
  return useQuery({
    queryKey: keys.actionList(query),
    enabled,
    queryFn: () => call<ActionPage>(api.GET("/api/v1/actions", { params: { query } })),
  });
}
