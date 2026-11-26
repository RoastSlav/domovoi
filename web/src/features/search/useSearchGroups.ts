import type {
  ExperienceSearch,
  Host,
  IncidentSummary,
  KnowledgeFact,
  NoteHit,
  OperationSummary,
} from "../../api/types";
import { useFactSearch } from "../../api/hooks/knowledge";
import { useExperienceSearch } from "../../api/hooks/experience";
import { useHostNames, useHosts } from "../../api/hooks/hosts";
import { useIncidents } from "../../api/hooks/incidents";
import { useNoteSearch } from "../../api/hooks/notes";
import { useOperations } from "../../api/hooks/operations";
import { matchesAll, words } from "./matching";

/** One hit of the hub's search over past operations. */
export type PastHit = ExperienceSearch["items"][number];

/** The fewest characters a search needs before anything is asked. */
export const MIN_QUERY = 2;

/** The groups of the page, in order, and who matches each: the hub or this browser. */
export const SEARCH_GROUPS = [
  { id: "incidents", label: "Incidents", matched: "hub" },
  { id: "operations", label: "Active operations", matched: "browser" },
  { id: "past", label: "Past operations", matched: "hub" },
  { id: "notes", label: "Notes", matched: "hub" },
  { id: "facts", label: "Facts", matched: "hub" },
  { id: "hosts", label: "Hosts", matched: "browser" },
] as const satisfies readonly {
  id: "incidents" | "operations" | "past" | "notes" | "facts" | "hosts";
  label: string;
  matched: "hub" | "browser";
}[];

/** What one group has so far. `total` is how many items matched; `items` are all of them. */
export interface GroupState<T> {
  error?: unknown;
  items: T[];
  /** Something to say about the group's coverage. */
  note?: string;
  retry: () => void;
  status: "idle" | "pending" | "error" | "ready";
  total: number;
}

/** Every group's state. `historyLabel` is the hub's statement about what past operations are. */
export interface SearchGroups {
  facts: GroupState<KnowledgeFact>;
  historyLabel?: string | undefined;
  hosts: GroupState<Host>;
  incidents: GroupState<IncidentSummary>;
  notes: GroupState<NoteHit>;
  operations: GroupState<OperationSummary>;
  past: GroupState<PastHit>;
}

/** The sentence that says what a search covers and who does the matching. */
export function scopeSentence(groups: readonly { label: string; matched: string }[]): string {
  const names = groups.map((group) => group.label.toLowerCase());
  const list = `${names.slice(0, -1).join(", ")} and ${names.at(-1) ?? ""}`;
  const here = groups
    .filter((group) => group.matched === "browser")
    .map((group) => group.label.toLowerCase())
    .join(" and ");
  const local = here.charAt(0).toUpperCase() + here.slice(1);
  return `Searches ${list}. ${local} are matched here against complete lists; the rest are searched by the hub.`;
}

const ACTIVE_STATUSES = [
  "queued",
  "running",
  "waiting_approval",
  "waiting_user",
  "waiting_agent",
  "waiting_timer",
  "paused",
] as const;

const NOT_ASKED = (): GroupState<never> => ({
  status: "idle",
  items: [],
  total: 0,
  retry: () => undefined,
});

interface Settled<T> {
  data: T | undefined;
  error: unknown;
  isError: boolean;
  isPending: boolean;
  refetch: () => unknown;
}

function stateOf<T, I>(
  active: boolean,
  query: Settled<T>,
  pick: (data: T) => I[],
  note?: string,
): GroupState<I> {
  if (!active) {
    return NOT_ASKED();
  }
  const retry = () => void query.refetch();
  if (query.isError) {
    return { status: "error", items: [], total: 0, error: query.error, retry };
  }
  if (query.isPending || query.data === undefined) {
    return { status: "pending", items: [], total: 0, retry };
  }
  const items = pick(query.data);
  return { status: "ready", items, total: items.length, retry, ...(note ? { note } : {}) };
}

/** Asks every source about `q` (trimmed) and returns each group's state. Idle under 2 characters. */
export function useSearchGroups(q: string): SearchGroups {
  const active = q.length >= MIN_QUERY;
  const wanted = words(q);
  const names = useHostNames();
  const hosts = useHosts();
  const incidents = useIncidents({ q }, active);
  const lists = [
    useOperations({ status: ACTIVE_STATUSES[0] }, active),
    useOperations({ status: ACTIVE_STATUSES[1] }, active),
    useOperations({ status: ACTIVE_STATUSES[2] }, active),
    useOperations({ status: ACTIVE_STATUSES[3] }, active),
    useOperations({ status: ACTIVE_STATUSES[4] }, active),
    useOperations({ status: ACTIVE_STATUSES[5] }, active),
    useOperations({ status: ACTIVE_STATUSES[6] }, active),
  ];
  const past = useExperienceSearch(active ? { q, scope: "global" } : undefined);
  const notes = useNoteSearch(active ? q : "", undefined);
  const facts = useFactSearch({ q }, active);

  const failed = lists.find((list) => list.isError);
  const waiting = lists.some((list) => list.isPending || list.data === undefined);
  const truncated = lists.some((list) => list.hasNextPage);
  const operations = stateOf(
    active,
    {
      data: lists,
      error: failed?.error,
      isError: failed !== undefined,
      isPending: waiting,
      refetch: () => lists.forEach((list) => list.isError && void list.refetch()),
    },
    (all) =>
      all
        .flatMap((list) => list.data?.pages.flatMap((page) => page.items) ?? [])
        .filter((operation) =>
          matchesAll(
            [operation.goal, ...operation.hosts.map((host) => names.get(host) ?? host)].join(" "),
            wanted,
          ),
        ),
    truncated ? "Only the newest 50 operations in each state were searched." : undefined,
  );
  return {
    incidents: stateOf(active, incidents, (data) => data.pages[0]?.items ?? []),
    operations,
    past: stateOf(active, past, (data) => data.items),
    historyLabel: active ? past.data?.label : undefined,
    notes: stateOf(active, notes, (data) => data),
    facts: stateOf(active, facts, (data) => data.items),
    hosts: stateOf(active, hosts, (data) => data.filter((host) => matchesAll(host.name, wanted))),
  };
}
