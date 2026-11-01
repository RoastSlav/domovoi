import { QueryClient } from "@tanstack/react-query";
import { ApiFailure } from "./types";

/** Query keys, so invalidation matches what the hooks cache. */
export const keys = {
  session: ["session"] as const,
  incidents: ["incidents"] as const,
  incidentList: (filters: object) => ["incidents", "list", filters] as const,
  incident: (id: string) => ["incidents", "detail", id] as const,
  actions: ["actions"] as const,
  actionList: (status: string | undefined) => ["actions", "list", status ?? "all"] as const,
  hosts: ["hosts"] as const,
  host: (name: string) => ["hosts", "detail", name] as const,
  skills: (name: string) => ["hosts", "skills", name] as const,
  notes: ["notes"] as const,
  noteList: (host: string | undefined) => ["notes", "list", host ?? ""] as const,
  noteSearch: (q: string, host: string | undefined) => ["notes", "search", q, host ?? ""] as const,
  note: (id: string) => ["notes", "detail", id] as const,
  settings: ["settings"] as const,
  advisories: ["advisories"] as const,
  digest: ["digest"] as const,
};

/** Retries once, but never a client error: a 401 or 404 will not change on retry. */
function retry(failures: number, error: unknown): boolean {
  if (error instanceof ApiFailure && error.status < 500) {
    return false;
  }
  return failures < 1;
}

/** A query client with the app's defaults. */
export function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry, staleTime: 10_000 },
      mutations: { retry: false },
    },
  });
}

/** The app's query client; the API client reads the session's CSRF token from it. */
export const queryClient = makeQueryClient();
