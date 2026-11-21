import { QueryClient } from "@tanstack/react-query";
import { ApiFailure } from "./types";

/** Query keys, so invalidation matches what the hooks cache. */
export const keys = {
  session: ["session"] as const,
  policy: ["policy"] as const,
  policyOverview: ["policy", "overview"] as const,
  policyVersions: (limit: number) => ["policy", "versions", limit] as const,
  policyVersion: (number: number) => ["policy", "version", number] as const,
  policyDraft: ["policy", "draft"] as const,
  policyPreview: (revision: number, rulesHash: string) =>
    ["policy", "preview", revision, rulesHash] as const,
  policyShadow: (filters: object) => ["policy", "shadow", filters] as const,
  policyDecision: (execution: string) => ["policy", "decision", execution] as const,
  preferences: ["preferences"] as const,
  incidents: ["incidents"] as const,
  incidentLists: ["incidents", "list"] as const,
  incidentDetails: ["incidents", "detail"] as const,
  incidentList: (filters: object) => ["incidents", "list", filters] as const,
  incident: (id: string) => ["incidents", "detail", id] as const,
  incidentChange: (id: string) => ["incidents", "change", id] as const,
  followUps: (id: string) => ["incidents", "followups", id] as const,
  ackRules: ["ack-rules"] as const,
  actions: ["actions"] as const,
  actionList: (filters: object) => ["actions", "list", filters] as const,
  overview: ["overview"] as const,
  overviewFor: (host: string | undefined) => ["overview", host ?? ""] as const,
  hosts: ["hosts"] as const,
  host: (name: string) => ["hosts", "detail", name] as const,
  skills: (name: string) => ["hosts", "skills", name] as const,
  capabilities: (name: string) => ["hosts", "capabilities", name] as const,
  hostCeiling: (name: string) => ["hosts", "ceiling", name] as const,
  hostPackages: (hostId: string) => ["hosts", "packages", hostId] as const,
  packagesStatus: ["packages", "status"] as const,
  packages: ["packages", "list"] as const,
  package: (name: string, version: string) => ["packages", "detail", name, version] as const,
  installJobs: ["packages", "jobs"] as const,
  installJob: (id: string) => ["packages", "jobs", id] as const,
  notes: ["notes"] as const,
  noteList: (host: string | undefined) => ["notes", "list", host ?? ""] as const,
  noteSearch: (q: string, host: string | undefined) => ["notes", "search", q, host ?? ""] as const,
  note: (id: string) => ["notes", "detail", id] as const,
  settings: ["settings"] as const,
  settingsSchema: ["settings", "schema"] as const,
  hostChecks: (name: string) => ["hosts", "checks", name] as const,
  hostDocker: (name: string) => ["hosts", "docker", name] as const,
  hostProfile: (name: string) => ["hosts", "profile", name] as const,
  hostLogs: (name: string, level: string, lines: number) =>
    ["hosts", "logs", name, level, lines] as const,
  advisories: ["advisories"] as const,
  digest: ["digest"] as const,
  update: ["system", "update"] as const,
  operations: ["operations"] as const,
  operation: (id: string) => ["operations", "detail", id] as const,
  operationList: (filters: object) => ["operations", "list", filters] as const,
  operationStep: (operation: string, step: string | undefined) =>
    ["operations", "detail", operation, "step", step ?? ""] as const,
  operationSummary: (id: string) => ["operations", "detail", id, "summary"] as const,
  operationSimilar: (id: string, limit: number | undefined) =>
    ["operations", "detail", id, "similar", limit ?? ""] as const,
  incidentSimilar: (id: string, limit: number | undefined) =>
    ["operations", "incident-similar", id, limit ?? ""] as const,
  experienceSearch: (params: object) => ["experience", "search", params] as const,
  knowledge: ["knowledge"] as const,
  facts: (filters: object) => ["knowledge", "facts", filters] as const,
  factSearch: (filters: object) => ["knowledge", "fact-search", filters] as const,
  fact: (id: string) => ["knowledge", "fact", id] as const,
  managementPaths: (host: string) => ["knowledge", "paths", host] as const,
  relations: (filters: object) => ["knowledge", "relations", filters] as const,
  entities: ["knowledge", "entities"] as const,
  conversations: ["conversations"] as const,
  conversationList: (filter: object) => ["conversations", "list", filter] as const,
  conversation: (id: string) => ["conversations", "detail", id] as const,
  logs: (level: string | undefined, lines: number | undefined) =>
    ["system", "logs", level ?? "", lines ?? ""] as const,
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
