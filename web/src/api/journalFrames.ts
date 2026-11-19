import type { QueryClient } from "@tanstack/react-query";
import { keys } from "./queryClient";

/** One row of the hub's journal as the event stream delivers it. */
export interface JournalFrame {
  seq: number;
  /** An open string: unknown aggregate types are ignored, never rejected. */
  aggregate_type: string;
  aggregate_id: string;
  /** An open string: unknown kinds are ignored, never rejected. */
  kind: string;
  payload_v: number;
  payload: Record<string, unknown> | null;
  payload_truncated?: boolean;
  actor: string;
  at: string;
  topics: string[];
}

/** The first frame of a connection: where the journal ends and whether the cursor was honoured. */
export interface ReadyFrame {
  head_seq: number;
  /** False when the stream starts from the head, so the client must refetch its snapshots. */
  resumed: boolean;
  topics: string[];
}

/** Tells the client its cursor is unusable and the stream restarts from `head_seq`. */
export interface ResetFrame {
  reason: "pruned" | "ahead" | "lagged" | string;
  head_seq: number;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function asObject(data: unknown): Record<string, unknown> | undefined {
  if (typeof data === "string") {
    try {
      const parsed: unknown = JSON.parse(data);
      return isRecord(parsed) ? parsed : undefined;
    } catch {
      return undefined;
    }
  }
  return isRecord(data) ? data : undefined;
}

/** Parses the data of a `journal` frame, or `undefined` when it is malformed. */
export function parseJournalFrame(data: unknown): JournalFrame | undefined {
  const raw = asObject(data);
  if (!raw) {
    return undefined;
  }
  const { seq, aggregate_type, aggregate_id, kind, payload } = raw;
  if (
    typeof seq !== "number" ||
    !Number.isFinite(seq) ||
    typeof aggregate_type !== "string" ||
    typeof aggregate_id !== "string" ||
    typeof kind !== "string"
  ) {
    return undefined;
  }
  const frame: JournalFrame = {
    seq,
    aggregate_type,
    aggregate_id,
    kind,
    payload_v: typeof raw.payload_v === "number" ? raw.payload_v : 1,
    payload: isRecord(payload) ? payload : null,
    actor: typeof raw.actor === "string" ? raw.actor : "",
    at: typeof raw.at === "string" ? raw.at : "",
    topics: Array.isArray(raw.topics)
      ? raw.topics.filter((topic): topic is string => typeof topic === "string")
      : [],
  };
  if (raw.payload_truncated === true) {
    frame.payload_truncated = true;
  }
  return frame;
}

/** Parses the data of a `ready` frame, or `undefined` when it is malformed. */
export function parseReadyFrame(data: unknown): ReadyFrame | undefined {
  const raw = asObject(data);
  if (!raw || typeof raw.head_seq !== "number") {
    return undefined;
  }
  return {
    head_seq: raw.head_seq,
    resumed: raw.resumed === true,
    topics: Array.isArray(raw.topics)
      ? raw.topics.filter((topic): topic is string => typeof topic === "string")
      : [],
  };
}

/** Parses the data of a `reset` frame, or `undefined` when it is malformed. */
export function parseResetFrame(data: unknown): ResetFrame | undefined {
  const raw = asObject(data);
  if (!raw || typeof raw.head_seq !== "number") {
    return undefined;
  }
  return { reason: typeof raw.reason === "string" ? raw.reason : "", head_seq: raw.head_seq };
}

type Key = readonly unknown[];

function olderThan(seq: number | undefined) {
  return (query: { state: { data: unknown } }): boolean =>
    seq === undefined ||
    ((query.state.data as { as_of_seq?: number } | undefined)?.as_of_seq ?? 0) < seq;
}

function invalidateAll(client: QueryClient, queryKeys: Key[], seq: number | undefined) {
  return Promise.all(
    queryKeys.map((queryKey) => client.invalidateQueries({ queryKey, predicate: olderThan(seq) })),
  ).then(() => undefined);
}

/**
 * Invalidates the cached queries a journal row makes stale. A query whose snapshot is already at
 * or past the row's `seq` is left alone.
 */
export async function applyJournal(client: QueryClient, frame: JournalFrame): Promise<void> {
  const stale: Key[] = [];
  switch (frame.aggregate_type) {
    case "incident":
      stale.push(keys.incidentLists, keys.incident(frame.aggregate_id), keys.overview);
      break;
    case "action": {
      stale.push(keys.actions, keys.incidentLists);
      const incident = frame.payload?.incident;
      if (typeof incident === "string") {
        stale.push(keys.incident(incident));
      }
      stale.push(keys.overview);
      break;
    }
    case "operation":
      stale.push(keys.operations, keys.operation(frame.aggregate_id), keys.overview);
      break;
    default:
      break;
  }
  await invalidateAll(client, stale, frame.seq);
}

/**
 * Refetches every live query after a gap. With `atLeast` (the journal head), only snapshots older
 * than the head are refetched.
 */
export async function refetchLive(client: QueryClient, atLeast?: number): Promise<void> {
  await invalidateAll(
    client,
    [keys.incidents, keys.actions, keys.overview, keys.operations],
    atLeast,
  );
}
