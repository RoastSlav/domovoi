import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, defined, type IncidentDetail, type IncidentPage, type TriageState } from "../types";

/** The incident list's filters, as kept in the URL. */
export interface IncidentFilters {
  status?: string | undefined;
  host?: string | undefined;
  severity?: string | undefined;
}

/** The page size of the incident list. */
export const INCIDENT_PAGE = 50;

/**
 * Incidents matching `filters`, newest first, a page at a time.
 * Stays idle while `enabled` is false, for a list the page is not showing.
 */
export function useIncidents(filters: IncidentFilters, enabled = true) {
  return useInfiniteQuery({
    queryKey: keys.incidentList(filters),
    enabled,
    queryFn: ({ pageParam }) =>
      call<IncidentPage>(
        api.GET("/api/v1/incidents", {
          params: { query: defined({ ...filters, cursor: pageParam, limit: INCIDENT_PAGE }) },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One incident with its report, card, timeline and actions. */
export function useIncident(id: string) {
  return useQuery({
    queryKey: keys.incident(id),
    queryFn: () =>
      call<IncidentDetail>(api.GET("/api/v1/incidents/{id}", { params: { path: { id } } })),
  });
}

function useRefreshIncidents() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: keys.incidents });
}

/** Acknowledges the incident. */
export function useAck(id: string) {
  const onSuccess = useRefreshIncidents();
  return useMutation({
    mutationFn: () => call(api.POST("/api/v1/incidents/{id}/ack", { params: { path: { id } } })),
    onSuccess,
  });
}

/** Mutes the incident for some minutes. */
export function useMute(id: string) {
  const onSuccess = useRefreshIncidents();
  return useMutation({
    mutationFn: (minutes: number) =>
      call(
        api.POST("/api/v1/incidents/{id}/mute", { params: { path: { id } }, body: { minutes } }),
      ),
    onSuccess,
  });
}

/** Ends the incident's mute. */
export function useUnmute(id: string) {
  const onSuccess = useRefreshIncidents();
  return useMutation({
    mutationFn: () => call(api.POST("/api/v1/incidents/{id}/unmute", { params: { path: { id } } })),
    onSuccess,
  });
}

/** Runs triage again on the incident and reports how it went. With `fresh`, the model runs
 * even when a cached report would fit. */
export function useRetryTriage(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (fresh: boolean) =>
      call<TriageState>(
        api.POST("/api/v1/incidents/{id}/triage", {
          params: { path: { id }, query: { fresh } },
        }),
      ),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.incident(id) });
      void client.invalidateQueries({ queryKey: keys.incidents });
    },
  });
}

/** Asks triage a follow-up question about the incident. */
export function useAsk(id: string) {
  return useMutation({
    mutationFn: (question: string) =>
      call<{ answer: string }>(
        api.POST("/api/v1/incidents/{id}/ask", { params: { path: { id } }, body: { question } }),
      ),
  });
}
