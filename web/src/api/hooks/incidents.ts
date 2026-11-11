import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
  type QueryKey,
} from "@tanstack/react-query";
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

/**
 * Refreshes what a change to incident `id` makes stale: its own page, the lists it appears
 * in and the navigation's counts. The follow-up conversation is not touched.
 */
function refreshIncident(client: QueryClient, id: string): Promise<unknown> {
  return Promise.all([
    client.invalidateQueries({ queryKey: keys.incident(id) }),
    client.invalidateQueries({ queryKey: keys.incidentLists }),
    client.invalidateQueries({ queryKey: keys.overview }),
  ]);
}

type ListData = InfiniteData<IncidentPage>;

/** What the cache held before an optimistic change, to put back if the hub refuses it. */
interface Snapshot {
  detail: IncidentDetail | undefined;
  lists: [QueryKey, ListData | undefined][];
}

/** Shows incident `id` with `status` in the cached detail and lists right away. */
async function showStatus(client: QueryClient, id: string, status: string): Promise<Snapshot> {
  await Promise.all([
    client.cancelQueries({ queryKey: keys.incident(id) }),
    client.cancelQueries({ queryKey: keys.incidentLists }),
  ]);
  const snapshot: Snapshot = {
    detail: client.getQueryData<IncidentDetail>(keys.incident(id)),
    lists: client.getQueriesData<ListData>({ queryKey: keys.incidentLists }),
  };
  if (snapshot.detail) {
    client.setQueryData<IncidentDetail>(keys.incident(id), {
      ...snapshot.detail,
      incident: { ...snapshot.detail.incident, status },
    });
  }
  client.setQueriesData<ListData>(
    { queryKey: keys.incidentLists },
    (list) =>
      list && {
        ...list,
        pages: list.pages.map((page) => ({
          ...page,
          items: page.items.map((item) => (item.id === id ? { ...item, status } : item)),
        })),
      },
  );
  return snapshot;
}

/** Puts back what `snapshot` saved. */
function restore(client: QueryClient, id: string, snapshot: Snapshot | undefined): void {
  if (!snapshot) {
    return;
  }
  client.setQueryData(keys.incident(id), snapshot.detail);
  for (const [queryKey, data] of snapshot.lists) {
    client.setQueryData(queryKey, data);
  }
}

/** Acknowledges the incident. It reads as acknowledged at once, and goes back to what it was
 * if the hub refuses. */
export function useAck(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => call(api.POST("/api/v1/incidents/{id}/ack", { params: { path: { id } } })),
    onMutate: () => showStatus(client, id, "acked"),
    onError: (_error, _input, snapshot) => restore(client, id, snapshot),
    onSettled: () => refreshIncident(client, id),
  });
}

/** Mutes the incident for some minutes. */
export function useMute(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (minutes: number) =>
      call(
        api.POST("/api/v1/incidents/{id}/mute", { params: { path: { id } }, body: { minutes } }),
      ),
    onSuccess: () => refreshIncident(client, id),
  });
}

/** Ends the incident's mute. */
export function useUnmute(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => call(api.POST("/api/v1/incidents/{id}/unmute", { params: { path: { id } } })),
    onSuccess: () => refreshIncident(client, id),
  });
}

/** Closes the incident as a known condition, with the reason the human gives. `ignore_for_minutes`
 * keeps the check quiet for that many minutes, 0 means always, and null creates no rule, so the
 * check alerts again the next time it fires. The rule list changes, so it refreshes with the incident. */
export function useDismiss(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (body: DismissInput) =>
      call(api.POST("/api/v1/incidents/{id}/dismiss", { params: { path: { id } }, body })),
    onSuccess: () => {
      void refreshIncident(client, id);
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
      void refreshIncident(client, id);
    },
  });
}

/** Asks triage a follow-up question about the incident. The remembered turns refresh with
 * its answer, and so does the incident, whose timeline gains a line. */
export function useAsk(id: string) {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (question: string) =>
      call<{ answer: string }>(
        api.POST("/api/v1/incidents/{id}/ask", { params: { path: { id } }, body: { question } }),
      ),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: keys.followUps(id) });
      void client.invalidateQueries({ queryKey: keys.incident(id) });
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
