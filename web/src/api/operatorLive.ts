import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useReducer } from "react";
import { keys } from "./queryClient";
import { useJournalStream, type Live } from "./journalStream";

/** One tool the Operator is using or used for the answer being written. */
export interface LiveTool {
  failed: boolean;
  state: "started" | "finished";
  tool: string;
}

/** The answer being written right now, for one turn. */
export interface LiveState {
  /** Increases whenever the model starts a new request or redaction rewrote sent text. */
  attempt: number;
  /** The whole safe text of the current attempt so far. */
  text: string;
  tools: LiveTool[];
  turn: string | null;
}

/** Nothing is being written. */
export const NO_LIVE: LiveState = { turn: null, attempt: 0, text: "", tools: [] };

/** One ephemeral event with its parsed JSON. */
export interface LiveFrame {
  data: unknown;
  name: string;
}

function str(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

/**
 * Folds one live event into the state. A frame for a turn other than the one shown is ignored,
 * unless it is the turn the thread says is in progress (a page opened mid-turn). A lower
 * `attempt` never overwrites a higher one. Malformed or unknown frames change nothing.
 */
export function liveReducer(
  state: LiveState,
  frame: LiveFrame,
  activeTurn: string | undefined,
): LiveState {
  const data = frame.data;
  if (typeof data !== "object" || data === null) {
    return state;
  }
  const record = data as Record<string, unknown>;
  const turn = str(record.turn);
  if (turn === undefined) {
    return state;
  }
  if (frame.name === "live.start") {
    return { turn, attempt: 0, text: "", tools: [] };
  }
  if (frame.name === "live.end") {
    return turn === state.turn ? NO_LIVE : state;
  }
  if (frame.name !== "live.text" && frame.name !== "live.tool") {
    return state;
  }
  let base = state;
  if (state.turn !== turn) {
    if (turn !== activeTurn) {
      return state;
    }
    base = { turn, attempt: 0, text: "", tools: [] };
  }
  if (frame.name === "live.text") {
    const attempt = record.attempt;
    const text = str(record.text);
    if (typeof attempt !== "number" || text === undefined || attempt < base.attempt) {
      return base;
    }
    return { ...base, attempt, text };
  }
  const tool = str(record.tool);
  const phase = record.state;
  if (tool === undefined || (phase !== "started" && phase !== "finished")) {
    return base;
  }
  const failed = record.failed === true;
  if (phase === "started") {
    return { ...base, tools: [...base.tools, { tool, state: "started", failed }] };
  }
  const open = base.tools.findLastIndex((t) => t.tool === tool && t.state === "started");
  const finished: LiveTool = { tool, state: "finished", failed };
  const tools =
    open === -1
      ? [...base.tools, finished]
      : base.tools.map((t, index) => (index === open ? finished : t));
  return { ...base, tools };
}

/**
 * Follows one conversation: refetches its thread when a journal row or `live.end` says it
 * changed, and keeps the text of the answer being written. The caller shows `live` only while
 * `live.turn` equals the thread's turn in progress.
 */
export function useConversationLive(
  id: string | undefined,
  activeTurn: string | undefined,
): { live: LiveState; status: Live } {
  const client = useQueryClient();
  const [live, dispatch] = useReducer(
    (state: LiveState, action: LiveFrame & { active: string | undefined }) =>
      liveReducer(state, action, action.active),
    NO_LIVE,
  );
  const refetch = useCallback(
    (seq?: number) => {
      if (id === undefined) {
        return;
      }
      const stale = (query: { state: { data: unknown } }): boolean =>
        seq === undefined ||
        ((query.state.data as { as_of_seq?: number } | undefined)?.as_of_seq ?? 0) < seq;
      void client.invalidateQueries({ queryKey: keys.conversation(id), predicate: stale });
      void client.invalidateQueries({ queryKey: ["conversations", "list"], predicate: stale });
    },
    [client, id],
  );
  const status = useJournalStream({
    topics: [`conversations:${id ?? ""}`],
    enabled: id !== undefined,
    onFrame: (frame) => refetch(frame.seq),
    onReset: () => refetch(),
    onLive: (name, data) => {
      dispatch({ name, data, active: activeTurn });
      if (name === "live.end") {
        refetch();
      }
    },
  });
  return { live, status };
}
