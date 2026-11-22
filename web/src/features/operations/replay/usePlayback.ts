import { useCallback, useEffect, useMemo, useReducer } from "react";
import type { OperationTrace } from "../../../api/operationTrace";
import type { OperationHistory } from "../../../api/types";
import { motionMs } from "../../../design/motion";
import { buildFrames, positionsOf, type Frame, type ReplayPosition } from "./frames";
import {
  FRAME_INTERVAL_MS,
  indexOfPosition,
  initialPlayback,
  playbackReducer,
  type PlaybackAction,
  type PlaybackState,
  type Speed,
} from "./playback";
import { traceAt } from "./traceAt";

/** True when the system asks for reduced motion: the layout token reads 0. Read at the moment it matters. */
export function readReduced(): boolean {
  return motionMs("layout") === 0;
}

/** What the replay bar and the graph need from a replay. */
export interface Playback {
  dispatch: (action: PlaybackAction) => void;
  /** The moment shown, when the position is a recorded one. */
  frame: Frame | undefined;
  frames: readonly Frame[];
  /** Where the position stands in `positions`. */
  index: number;
  live: () => void;
  next: () => void;
  pause: () => void;
  play: () => void;
  positions: readonly ReplayPosition[];
  previous: () => void;
  /** The system asks for reduced motion: no automatic replay. */
  reduced: boolean;
  /** The trace as it was at the moment shown; undefined at live, closed or without a history. */
  replayTrace: OperationTrace | undefined;
  seek: (index: number) => void;
  setSpeed: (speed: Speed) => void;
  state: PlaybackState;
}

/**
 * The replay of one operation: the playback state, the moments the history gives, the trace at
 * the moment shown, and the timer that plays on. One timeout runs per tick while playing and
 * none while paused; it is cleared on a change and on unmount. Reduced motion is read at each
 * tick, so turning it on stops playback.
 */
export function usePlayback(
  trace: OperationTrace,
  history: OperationHistory | undefined,
): Playback {
  const [state, dispatch] = useReducer(playbackReducer, initialPlayback);
  const frames = useMemo(() => (history ? buildFrames(history.events) : []), [history]);
  const endsAtLive =
    history?.incomplete_reason !== "capped" && history?.incomplete_reason !== "both";
  const positions = useMemo(() => positionsOf(frames, endsAtLive), [frames, endsAtLive]);

  useEffect(() => {
    if (history) {
      dispatch({ type: "positions", positions });
    }
  }, [history, positions]);

  const { playing, position, speed } = state;
  useEffect(() => {
    if (!playing) {
      return undefined;
    }
    const timer = setTimeout(
      () => dispatch({ type: "tick", positions, reduced: readReduced() }),
      FRAME_INTERVAL_MS / speed,
    );
    return () => clearTimeout(timer);
  }, [playing, position, speed, positions]);

  const atMs = position.kind === "frame" ? position.atMs : undefined;
  const replayTrace = useMemo(
    () =>
      state.open && history && atMs !== undefined
        ? traceAt({ final: trace, history, frames, atMs })
        : undefined,
    [state.open, history, frames, atMs, trace],
  );
  const index = indexOfPosition(positions, position);
  const frame = atMs === undefined ? undefined : frames.find((f) => f.atMs === atMs);

  const previous = useCallback(() => dispatch({ type: "previous", positions }), [positions]);
  const next = useCallback(() => dispatch({ type: "next", positions }), [positions]);
  const play = useCallback(
    () => dispatch({ type: "play", positions, reduced: readReduced() }),
    [positions],
  );
  const pause = useCallback(() => dispatch({ type: "pause" }), []);
  const live = useCallback(() => dispatch({ type: "live" }), []);
  const seek = useCallback(
    (to: number) => {
      const target = positions[Math.min(positions.length - 1, Math.max(0, to))];
      if (target) {
        dispatch({ type: "seek", position: target });
      }
    },
    [positions],
  );
  const setSpeed = useCallback((to: Speed) => dispatch({ type: "speed", speed: to }), []);

  return {
    dispatch,
    frame,
    frames,
    index,
    live,
    next,
    pause,
    play,
    positions,
    previous,
    reduced: readReduced(),
    replayTrace,
    seek,
    setSpeed,
    state,
  };
}
