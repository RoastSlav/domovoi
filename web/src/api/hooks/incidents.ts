import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type DismissInput,
  type FollowUps,
  type IncidentDetail,
  type IncidentPage,
  type TriageState,
} from "../types";

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

/** The follow-up turns the model still remembers for the incident, oldest first. */
export function useFollowUps(id: string) {
  return useQuery({
    queryKey: keys.followUps(id),
    queryFn: () =>
      call<FollowUps>(api.GET("/api/v1/incidents/{id}/followups", { params: { path: { id } } })),
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

/** Closes the incident as a known condition, with the reason the human gives. `ignore_for_minutes`
 * keeps the check quiet for that many minutes, 0 means always, and null creates no rule, so the
 * check alerts again the next time it fires. The rule list changes, so it refreshes with the incidents. */
export function useDismiss(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: DismissInput) =>
      call(api.POST("/api/v1/incidents/{id}/dismiss", { params: { path: { id } }, body })),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.incidents });
      void client.invalidateQueries({ queryKey: keys.ackRules });
    },
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

/** Asks triage a follow-up question about the incident; the remembered turns refresh with
 * its answer. */
export function useAsk(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (question: string) =>
      call<{ answer: string }>(
        api.POST("/api/v1/incidents/{id}/ask", { params: { path: { id } }, body: { question } }),
      ),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.followUps(id) });
    },
  });
}

/** Forgets the follow-up conversation: the next question starts a new one. The incident detail
 * refreshes too, since the clearing writes one timeline line. */
export function useClearFollowUps(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () =>
      call(
        api.POST("/api/v1/incidents/{id}/followups/clear", {
          params: { path: { id } },
        }),
      ),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.followUps(id) });
      void client.invalidateQueries({ queryKey: keys.incident(id) });
    },
  });
}
