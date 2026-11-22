import type { ReplayPosition } from "./frames";

/** How fast playback runs, as a multiple of one moment per second. */
export type Speed = 1 | 2 | 4;

/** Everything the replay bar remembers; only in memory. */
export interface PlaybackState {
  open: boolean;
  playing: boolean;
  position: ReplayPosition;
  /** Bumped on each deliberate jump or restart, so the graph can animate again. */
  session: number;
  speed: Speed;
}

/** Closed, at live, not playing, 1x. */
export const initialPlayback: PlaybackState = {
  open: false,
  position: { kind: "live" },
  playing: false,
  speed: 1,
  session: 0,
};

/** The pause between moments at 1x; the timer waits this divided by the speed. */
export const FRAME_INTERVAL_MS = 1000;

/** Everything that can happen to a replay. */
export type PlaybackAction =
  | { type: "open" }
  | { type: "close" }
  | { type: "live" }
  | { type: "pause" }
  | { type: "speed"; speed: Speed }
  | { type: "seek"; position: ReplayPosition }
  | { type: "previous"; positions: readonly ReplayPosition[] }
  | { type: "next"; positions: readonly ReplayPosition[] }
  | { type: "play"; positions: readonly ReplayPosition[]; reduced: boolean }
  | { type: "tick"; positions: readonly ReplayPosition[]; reduced: boolean }
  | { type: "positions"; positions: readonly ReplayPosition[] };

/** Index of a position in the list: live is last; a vanished moment maps to the last earlier one. */
export function indexOfPosition(
  positions: readonly ReplayPosition[],
  position: ReplayPosition,
): number {
  if (position.kind === "live") {
    return Math.max(0, positions.length - 1);
  }
  let found = 0;
  for (let i = 0; i < positions.length; i += 1) {
    const p = positions[i];
    if (p?.kind === "frame" && p.atMs <= position.atMs) {
      found = i;
      if (p.atMs === position.atMs) {
        return i;
      }
    }
  }
  return found;
}

function same(a: ReplayPosition, b: ReplayPosition): boolean {
  return a.kind === b.kind && (a.kind === "live" || (b.kind === "frame" && a.atMs === b.atMs));
}

/** The playback rules; pure. */
export function playbackReducer(state: PlaybackState, action: PlaybackAction): PlaybackState {
  switch (action.type) {
    case "open":
      return { ...state, open: true, session: state.session + 1 };
    case "close":
      return {
        ...state,
        open: false,
        position: { kind: "live" },
        playing: false,
        session: state.session + 1,
      };
    case "live":
      return { ...state, position: { kind: "live" }, playing: false, session: state.session + 1 };
    case "pause":
      return { ...state, playing: false };
    case "speed":
      return { ...state, speed: action.speed };
    case "seek":
      return { ...state, position: action.position, playing: false, session: state.session + 1 };
    case "previous":
    case "next": {
      const { positions } = action;
      const step = action.type === "next" ? 1 : -1;
      const index = indexOfPosition(positions, state.position);
      const target = positions[Math.min(positions.length - 1, Math.max(0, index + step))];
      return { ...state, position: target ?? state.position, playing: false };
    }
    case "play": {
      if (action.reduced || action.positions.length < 2) {
        return state;
      }
      const last = action.positions.length - 1;
      if (indexOfPosition(action.positions, state.position) >= last) {
        const first = action.positions[0];
        return {
          ...state,
          position: first ?? state.position,
          playing: true,
          session: state.session + 1,
        };
      }
      return { ...state, playing: true };
    }
    case "tick": {
      if (!state.playing) {
        return state;
      }
      if (action.reduced) {
        return { ...state, playing: false };
      }
      const last = action.positions.length - 1;
      const next = indexOfPosition(action.positions, state.position) + 1;
      if (next >= last) {
        return { ...state, position: { kind: "live" }, playing: false };
      }
      return { ...state, position: action.positions[next] ?? state.position };
    }
    case "positions": {
      if (action.positions.length === 0 || state.position.kind === "live") {
        return state;
      }
      const resolved = action.positions[indexOfPosition(action.positions, state.position)];
      if (resolved === undefined || same(resolved, state.position)) {
        return state;
      }
      return { ...state, position: resolved, session: state.session + 1 };
    }
  }
}
