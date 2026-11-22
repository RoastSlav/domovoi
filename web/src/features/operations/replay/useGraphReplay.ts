import { useEffect, useState } from "react";
import type { OperationTrace } from "../../../api/operationTrace";
import { useOperationHistory } from "../../../api/hooks/operationHistory";
import type { OperationHistory } from "../../../api/types";
import { formatClock } from "./clock";
import type { HistoryLoad } from "./ReplayBar";
import { NOT_REPLAYING_STATUS, useSetReplayStatus } from "./ReplayContext";
import { usePlayback, type Playback } from "./usePlayback";

/** Everything the Graph view needs to offer a replay. */
export interface GraphReplay {
  history: OperationHistory | undefined;
  load: HistoryLoad;
  open: boolean;
  playback: Playback;
  /** Opens the bar (fetching the history) or closes it and returns to live. */
  toggle: () => void;
}

/**
 * The replay of one operation's graph: the on-demand history, the playback over it, and the
 * replay status the live panels read. The bar starts closed and fetches nothing; the status is
 * cleared when the replay stops and when the view unmounts.
 */
export function useGraphReplay(trace: OperationTrace): GraphReplay {
  const [open, setOpen] = useState(false);
  const query = useOperationHistory(trace.operation.id, open);
  const playback = usePlayback(trace, query.data);
  const { dispatch } = playback;
  const setStatus = useSetReplayStatus();
  const replaying = open && playback.replayTrace !== undefined;
  const label = playback.frame ? formatClock(playback.frame.atMs) : "";

  useEffect(() => {
    setStatus(replaying ? { replaying: true, label } : NOT_REPLAYING_STATUS);
  }, [replaying, label, setStatus]);
  useEffect(() => () => setStatus(NOT_REPLAYING_STATUS), [setStatus]);

  const toggle = (): void => {
    dispatch({ type: open ? "close" : "open" });
    setOpen(!open);
  };
  let load: HistoryLoad = { kind: "ready" };
  if (query.data === undefined) {
    load = query.isError
      ? { kind: "failed", error: query.error, retry: query.refetch }
      : { kind: "loading" };
  }
  return { history: query.data, load, open, playback, toggle };
}
