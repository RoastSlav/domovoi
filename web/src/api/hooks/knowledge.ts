import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type KnowledgeEntity,
  type KnowledgeFact,
  type KnowledgeFactPage,
  type KnowledgeRelationPage,
  type ManagementPaths,
} from "../types";

/** The page size of the fact and relation lists. */
export const FACT_PAGE = 50;

/** What the fact list narrows by; `status` is a comma list and its absence means every live status. */
export interface FactFilters {
  host?: string | undefined;
  subject?: string | undefined;
  predicate?: string | undefined;
  source?: string | undefined;
  status?: string | undefined;
}

/** Facts matching `filters`, newest first, a page at a time; idle while `enabled` is false. */
export function useFacts(filters: FactFilters, enabled = true) {
  return useInfiniteQuery({
    queryKey: keys.facts(filters),
    enabled,
    queryFn: ({ pageParam }) =>
      call<KnowledgeFactPage>(
        api.GET("/api/v1/knowledge/facts", {
          params: { query: defined({ ...filters, cursor: pageParam, limit: FACT_PAGE }) },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** The best matches for a text, at most 50 and with no next page; idle while the text is blank. */
export function useFactSearch(filters: FactFilters & { q: string }, enabled = true) {
  return useQuery({
    queryKey: keys.factSearch(filters),
    enabled: enabled && filters.q.trim() !== "",
    queryFn: () =>
      call<KnowledgeFactPage>(
        api.GET("/api/v1/knowledge/facts", {
          params: { query: defined({ ...filters, q: filters.q.trim(), limit: 50 }) },
        }),
      ),
  });
}

/** One fact by id. */
export function useFact(id: string) {
  return useQuery({
    queryKey: keys.fact(id),
    queryFn: (): Promise<KnowledgeFact> =>
      call<KnowledgeFact>(api.GET("/api/v1/knowledge/facts/{id}", { params: { path: { id } } })),
  });
}

/** A host's management paths and whether coverage can be checked; idle without a host name. */
export function useManagementPaths(host: string | undefined) {
  return useQuery({
    queryKey: keys.managementPaths(host ?? ""),
    enabled: host !== undefined,
    queryFn: () =>
      call<ManagementPaths>(
        api.GET("/api/v1/knowledge/hosts/{name}/management-paths", {
          params: { path: { name: host ?? "" } },
        }),
      ),
  });
}

/** What the relation list narrows by. */
export interface RelationFilters {
  subject?: string | undefined;
  kind?: string | undefined;
  include_retracted?: boolean | undefined;
}

/** Relations matching `filters`, newest first, a page at a time; retracted ones only when asked for. */
export function useRelations(filters: RelationFilters, enabled = true) {
  const { include_retracted, ...rest } = filters;
  return useInfiniteQuery({
    queryKey: keys.relations(filters),
    enabled,
    queryFn: ({ pageParam }) =>
      call<KnowledgeRelationPage>(
        api.GET("/api/v1/knowledge/relations", {
          params: {
            query: defined({
              ...rest,
              include_retracted: include_retracted ? true : undefined,
              cursor: pageParam,
              limit: FACT_PAGE,
            }),
          },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** Every entity the owner named; the list is not paged. */
export function useEntities() {
  return useQuery({
    queryKey: keys.entities,
    queryFn: () => call<KnowledgeEntity[]>(api.GET("/api/v1/knowledge/entities", {})),
  });
}
