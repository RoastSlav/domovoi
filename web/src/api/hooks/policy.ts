import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type PolicyDraft,
  type PolicyOverview,
  type PolicyPreview,
  type PolicyVersionDetail,
  type PolicyVersionPage,
  type ShadowDetail,
  type ShadowPage,
} from "../types";

/** The page size of the version list. */
const VERSION_PAGE = 25;

/** The page size of the recorded decision list. */
export const SHADOW_PAGE = 25;

/** The recorded decision list's filters, as kept in the URL. */
export interface ShadowFilters {
  host?: string | undefined;
  contract?: string | undefined;
  status?: string | undefined;
  outcome?: string | undefined;
  differing?: boolean | undefined;
  since?: string | undefined;
  until?: string | undefined;
}

/** The policy at a glance: active version, draft and the shadow recorder. */
export function usePolicyOverview() {
  return useQuery({
    queryKey: keys.policyOverview,
    queryFn: () => call<PolicyOverview>(api.GET("/api/v1/policy")),
  });
}

/** Published versions, newest first, a page at a time. */
export function usePolicyVersions(limit: number = VERSION_PAGE) {
  return useInfiniteQuery({
    queryKey: keys.policyVersions(limit),
    queryFn: ({ pageParam }) =>
      call<PolicyVersionPage>(
        api.GET("/api/v1/policy/versions", {
          params: { query: defined({ cursor: pageParam, limit }) },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One published version with its rules; idle until a number is given. */
export function usePolicyVersion(number: number | undefined) {
  return useQuery({
    queryKey: keys.policyVersion(number ?? -1),
    enabled: number !== undefined,
    queryFn: () =>
      call<PolicyVersionDetail>(
        api.GET("/api/v1/policy/versions/{number}", {
          params: { path: { number: number ?? 0 } },
        }),
      ),
  });
}

/** The stored draft; idle while `enabled` is false. A missing draft answers 404. */
export function usePolicyDraft(enabled: boolean) {
  return useQuery({
    queryKey: keys.policyDraft,
    enabled,
    queryFn: () => call<PolicyDraft>(api.GET("/api/v1/policy/draft")),
  });
}

/**
 * The review of the stored draft against the active version; idle until there is a
 * draft. It is keyed by the draft's revision and rules fingerprint and never reused.
 */
export function useDraftPreview(draft: PolicyDraft | undefined) {
  return useQuery({
    queryKey: keys.policyPreview(draft?.revision ?? -1, draft?.rules_hash ?? ""),
    enabled: draft !== undefined,
    staleTime: 0,
    refetchOnWindowFocus: false,
    queryFn: () =>
      call<PolicyPreview>(
        api.POST("/api/v1/policy/preview", {
          body: { base_revision: draft?.revision ?? null, rules: draft?.rules ?? [] },
        }),
      ),
  });
}

/** Recorded decisions matching `filters`, newest first, a page at a time. */
export function useShadowDecisions(filters: ShadowFilters, enabled = true) {
  return useInfiniteQuery({
    queryKey: keys.policyShadow(filters),
    enabled,
    queryFn: ({ pageParam }) =>
      call<ShadowPage>(
        api.GET("/api/v1/policy/shadow", {
          params: {
            query: defined({
              ...filters,
              differing: filters.differing ? true : undefined,
              cursor: pageParam,
              limit: SHADOW_PAGE,
            }),
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One recorded decision with its inputs; idle until an execution is given. */
export function useShadowDecision(execution: string | undefined) {
  return useQuery({
    queryKey: keys.policyDecision(execution ?? ""),
    enabled: execution !== undefined,
    queryFn: () =>
      call<ShadowDetail>(
        api.GET("/api/v1/policy/shadow/{execution_id}", {
          params: { path: { execution_id: execution ?? "" } },
        }),
      ),
  });
}
