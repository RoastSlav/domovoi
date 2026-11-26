import { useActions } from "../../api/hooks/actions";
import { useHosts } from "../../api/hooks/hosts";
import { useIncidents } from "../../api/hooks/incidents";
import { useOperations } from "../../api/hooks/operations";
import type { Action, Host, IncidentSummary, OperationSummary } from "../../api/types";
import { useContainmentSources } from "../containment/useContainmentSources";
import type { ContainmentAttentionInput } from "../containment/containmentItems";
import { attentionItems, type AttentionInputs, type AttentionItem } from "./attentionModel";

/** The lists the attention items come from, named as a person would say them. */
export type SourceName =
  | "incidents"
  | "approvals"
  | "unknown results"
  | "operations waiting for approval"
  | "operations waiting for an answer"
  | "paused operations"
  | "hosts"
  | "blocks";

/** A source whose latest answer is an error and that has nothing to show. */
export interface SourceFailure {
  name: SourceName;
  error: unknown;
  retry: () => void;
}

/** What the pages need to draw the attention list honestly. */
export interface AttentionState {
  /** From every source that loaded; empty until something loaded. */
  items: AttentionItem[];
  /** At least one source has never answered. */
  pending: boolean;
  failed: SourceFailure[];
  /** Sources with more pages than the first. */
  truncated: SourceName[];
  /** Every source answered successfully. */
  allLoaded: boolean;
  inputs: AttentionInputs;
}

/** What one source tells `summarizeSources`: its first page and how the request stands. */
export interface SourcePart<T> {
  name: SourceName;
  rows: readonly T[] | undefined;
  isError: boolean;
  error: unknown;
  isPending: boolean;
  hasMore: boolean;
  retry: () => void;
}

/** The seven sources. */
export interface SourcesParts {
  incidents: SourcePart<IncidentSummary>;
  proposed: SourcePart<Action>;
  unknownActions: SourcePart<Action>;
  waitingApproval: SourcePart<OperationSummary>;
  waitingUser: SourcePart<OperationSummary>;
  paused: SourcePart<OperationSummary>;
  hosts: SourcePart<Host>;
  /** Optional so a caller with no containment source still builds. */
  containment?: SourcePart<ContainmentAttentionInput>;
  hostNames: ReadonlyMap<string, string>;
}

/** Pure core of `useAttentionSources`: builds the items from what loaded and says what did not. */
export function summarizeSources(parts: SourcesParts): AttentionState {
  const all: SourcePart<unknown>[] = [
    parts.incidents,
    parts.proposed,
    parts.unknownActions,
    parts.waitingApproval,
    parts.waitingUser,
    parts.paused,
    parts.hosts,
    ...(parts.containment ? [parts.containment] : []),
  ];
  const inputs: AttentionInputs = {
    incidents: parts.incidents.rows ?? [],
    proposed: parts.proposed.rows ?? [],
    unknownActions: parts.unknownActions.rows ?? [],
    waitingApproval: parts.waitingApproval.rows ?? [],
    waitingUser: parts.waitingUser.rows ?? [],
    paused: parts.paused.rows ?? [],
    hosts: parts.hosts.rows ?? [],
    containment: parts.containment?.rows ?? [],
    hostNames: parts.hostNames,
  };
  return {
    items: attentionItems(inputs),
    pending: all.some((part) => part.rows === undefined && !part.isError),
    failed: all
      .filter((part) => part.isError && part.rows === undefined)
      .map(({ name, error, retry }) => ({ name, error, retry })),
    truncated: all.filter((part) => part.hasMore).map((part) => part.name),
    allLoaded: all.every((part) => part.rows !== undefined),
    inputs,
  };
}

interface QueryLike {
  error: unknown;
  isError: boolean;
  isPending: boolean;
  refetch: () => unknown;
}

function part<T>(
  name: SourceName,
  query: QueryLike,
  rows: readonly T[] | undefined,
  hasMore = false,
): SourcePart<T> {
  return {
    name,
    rows,
    isError: query.isError,
    error: query.error,
    isPending: query.isPending,
    hasMore,
    retry: () => void query.refetch(),
  };
}

/**
 * Every source of the attention list, for all hosts or the chosen one. Only the first page of
 * each is read; a source that failed with nothing to show is reported, never read as empty.
 */
export function useAttentionSources(host: string | undefined): AttentionState {
  const incidents = useIncidents({ host, group: "needs_action" });
  const proposed = useActions({ status: "proposed", host });
  const unknown = useActions({ status: "outcome_unknown", host });
  const waitingApproval = useOperations({ status: "waiting_approval", host });
  const waitingUser = useOperations({ status: "waiting_user", host });
  const paused = useOperations({ status: "paused", host });
  const hosts = useHosts();
  const names = new Map((hosts.data ?? []).map((one) => [one.id, one.name]));
  const blocks = useContainmentSources(
    hosts.data?.filter((one) => host === undefined || one.name === host),
  );
  return summarizeSources({
    incidents: part("incidents", incidents, incidents.data?.pages[0]?.items, incidents.hasNextPage),
    proposed: part("approvals", proposed, proposed.data?.items),
    unknownActions: part("unknown results", unknown, unknown.data?.items),
    waitingApproval: part(
      "operations waiting for approval",
      waitingApproval,
      waitingApproval.data?.pages[0]?.items,
      waitingApproval.hasNextPage,
    ),
    waitingUser: part(
      "operations waiting for an answer",
      waitingUser,
      waitingUser.data?.pages[0]?.items,
      waitingUser.hasNextPage,
    ),
    paused: part("paused operations", paused, paused.data?.pages[0]?.items, paused.hasNextPage),
    hosts: part(
      "hosts",
      hosts,
      hosts.data?.filter((one) => host === undefined || one.name === host),
    ),
    containment: {
      name: "blocks",
      rows: blocks.rows,
      isError: blocks.isError,
      error: blocks.error,
      isPending: blocks.isPending,
      hasMore: false,
      retry: blocks.retry,
    },
    hostNames: names,
  });
}
