import type { ReplayPosition } from "./frames";
import {
  FRAME_INTERVAL_MS,
  initialPlayback,
  playbackReducer,
  type PlaybackState,
} from "./playback";

const f = (atMs: number): ReplayPosition => ({ kind: "frame", atMs });
const live: ReplayPosition = { kind: "live" };
const three: ReplayPosition[] = [f(1000), f(2000), live];
const four: ReplayPosition[] = [f(1000), f(2000), f(3000), live];
const at = (position: ReplayPosition, extra: Partial<PlaybackState> = {}): PlaybackState => ({
  ...initialPlayback,
  open: true,
  position,
  ...extra,
});

test("the_initial_state_is_closed_at_live_and_not_playing", () => {
  expect(initialPlayback).toEqual({
    open: false,
    position: live,
    playing: false,
    speed: 1,
    session: 0,
  });
  expect(FRAME_INTERVAL_MS).toBe(1000);
});

test("open_keeps_live_and_bumps_the_session", () => {
  expect(playbackReducer(initialPlayback, { type: "open" })).toEqual({
    ...initialPlayback,
    open: true,
    session: 1,
  });
});

test("close_returns_to_live_and_stops", () => {
  const next = playbackReducer(at(f(1000), { playing: true, session: 4 }), { type: "close" });
  expect(next).toMatchObject({ open: false, position: live, playing: false, session: 5 });
});

test("live_returns_to_live_and_stops", () => {
  const next = playbackReducer(at(f(1000), { playing: true, session: 2 }), { type: "live" });
  expect(next).toMatchObject({ open: true, position: live, playing: false, session: 3 });
});

test("pause_stops_without_moving", () => {
  const next = playbackReducer(at(f(2000), { playing: true, session: 2 }), { type: "pause" });
  expect(next).toEqual(at(f(2000), { session: 2 }));
});

test("seek_pauses_and_bumps_the_session", () => {
  const next = playbackReducer(at(live, { playing: true }), { type: "seek", position: f(2000) });
  expect(next).toMatchObject({ position: f(2000), playing: false, session: 1 });
});

test("speed_changes_the_delay_not_the_position", () => {
  const next = playbackReducer(at(f(1000), { playing: true }), { type: "speed", speed: 4 });
  expect(next).toEqual(at(f(1000), { playing: true, speed: 4 }));
});

test("previous_steps_back_and_stops_at_the_first_position", () => {
  const back = playbackReducer(at(live, { playing: true, session: 3 }), {
    type: "previous",
    positions: three,
  });
  expect(back).toMatchObject({ position: f(2000), playing: false, session: 3 });
  const first = playbackReducer(at(f(1000)), { type: "previous", positions: three });
  expect(first.position).toEqual(f(1000));
});

test("next_reaches_live_and_stays", () => {
  const forward = playbackReducer(at(f(1000)), { type: "next", positions: three });
  expect(forward.position).toEqual(f(2000));
  const toLive = playbackReducer(forward, { type: "next", positions: three });
  expect(toLive.position).toEqual(live);
  expect(playbackReducer(toLive, { type: "next", positions: three }).position).toEqual(live);
});

test("previous_from_live_with_only_live_stays", () => {
  expect(playbackReducer(at(live), { type: "previous", positions: [live] }).position).toEqual(live);
});

test("play_at_live_restarts_from_the_first_position", () => {
  const next = playbackReducer(at(live, { session: 1 }), {
    type: "play",
    positions: three,
    reduced: false,
  });
  expect(next).toMatchObject({ position: f(1000), playing: true, session: 2 });
});

test("play_from_the_middle_continues_without_a_new_session", () => {
  const next = playbackReducer(at(f(2000), { session: 1 }), {
    type: "play",
    positions: four,
    reduced: false,
  });
  expect(next).toMatchObject({ position: f(2000), playing: true, session: 1 });
});

test("play_does_nothing_under_reduced_motion", () => {
  const state = at(f(1000));
  expect(playbackReducer(state, { type: "play", positions: three, reduced: true })).toBe(state);
});

test("play_does_nothing_with_fewer_than_two_positions", () => {
  const state = at(live);
  expect(playbackReducer(state, { type: "play", positions: [live], reduced: false })).toBe(state);
});

test("tick_advances_and_stops_at_live", () => {
  let state = at(f(1000), { playing: true });
  state = playbackReducer(state, { type: "tick", positions: four, reduced: false });
  expect(state).toMatchObject({ position: f(2000), playing: true });
  state = playbackReducer(state, { type: "tick", positions: four, reduced: false });
  expect(state).toMatchObject({ position: f(3000), playing: true });
  state = playbackReducer(state, { type: "tick", positions: four, reduced: false });
  expect(state).toMatchObject({ position: live, playing: false });
});

test("tick_does_nothing_while_paused", () => {
  const state = at(f(1000));
  expect(playbackReducer(state, { type: "tick", positions: three, reduced: false })).toBe(state);
});

test("tick_stops_when_reduced_motion_turns_on", () => {
  const next = playbackReducer(at(f(1000), { playing: true }), {
    type: "tick",
    positions: three,
    reduced: true,
  });
  expect(next).toMatchObject({ position: f(1000), playing: false });
});

test("positions_snaps_a_vanished_moment_to_the_nearest_earlier_one", () => {
  const next = playbackReducer(at(f(2500), { playing: true, session: 1 }), {
    type: "positions",
    positions: four,
  });
  expect(next).toMatchObject({ position: f(2000), playing: true, session: 2 });
  const beforeAll = playbackReducer(at(f(500), { session: 1 }), {
    type: "positions",
    positions: four,
  });
  expect(beforeAll).toMatchObject({ position: f(1000), session: 2 });
});

test("positions_keeps_the_moment_and_the_session_when_nothing_changed", () => {
  const state = at(f(2000), { session: 7 });
  expect(playbackReducer(state, { type: "positions", positions: four })).toBe(state);
  const atLive = at(live, { session: 7 });
  expect(playbackReducer(atLive, { type: "positions", positions: four })).toBe(atLive);
});
