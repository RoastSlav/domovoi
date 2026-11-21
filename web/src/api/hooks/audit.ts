import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, defined, type AuditPage, type AuditVerification } from "../types";

/** Entries per request. */
export const AUDIT_PAGE = 50;

/** Which part of the audit history a list covers. */
export type AuditHistoryName = "chained" | "legacy";

/** One part of the audit history, newest first, a page at a time. */
export function useAuditRows(history: AuditHistoryName) {
  return useInfiniteQuery({
    queryKey: keys.auditRows(history),
    queryFn: ({ pageParam }) =>
      call<AuditPage>(
        api.GET("/api/v1/audit", {
          params: { query: defined({ history, cursor: pageParam, limit: AUDIT_PAGE }) },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/**
 * A check of the audit log. `recent` is cheap and runs on open; `full` reads everything and
 * is asked for by the person (`enabled`), never repeated on focus.
 */
export function useAuditVerification(scope: "recent" | "full", enabled = true) {
  return useQuery({
    queryKey: keys.auditVerification(scope),
    enabled,
    refetchOnWindowFocus: false,
    ...(scope === "full" ? { staleTime: Infinity } : {}),
    queryFn: () =>
      call<AuditVerification>(
        api.GET("/api/v1/audit/verification", { params: { query: { scope } } }),
      ),
  });
}
