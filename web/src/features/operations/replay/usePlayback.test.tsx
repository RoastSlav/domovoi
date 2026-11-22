import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { motionStub } from "../../../test/motionStub";
import { historyFor, must, TRACE_FIXTURES } from "../../../test/replayFixtures";
import { FRAME_INTERVAL_MS } from "./playback";
import { usePlayback } from "./usePlayback";

vi.mock("../../../design/motion", async (original) =>
  (await import("../../../test/motionStub")).stubbedMotion(await original()),
);

const trace = must(TRACE_FIXTURES["verified-success"]);
const history = historyFor(trace);

function setup() {
  const view = renderHook(() => usePlayback(trace, history));
  act(() => view.result.current.dispatch({ type: "open" }));
  return view;
}

beforeEach(() => {
  motionStub.ms = 240;
  vi.useFakeTimers();
});
afterEach(() => vi.useRealTimers());

describe("usePlayback", () => {
  it("the_fixture_has_enough_moments_to_play", () => {
    expect(setup().result.current.positions.length).toBeGreaterThan(3);
  });

  it.each([1, 2, 4] as const)(
    "playing_advances_one_position_per_interval_at_each_speed (%dx)",
    (speed) => {
      const { result } = setup();
      const delay = FRAME_INTERVAL_MS / speed;
      act(() => result.current.seek(0));
      act(() => result.current.setSpeed(speed));
      act(() => result.current.play());
      expect(result.current.state.playing).toBe(true);
      expect(result.current.index).toBe(0);
      act(() => void vi.advanceTimersByTime(delay - 1));
      expect(result.current.index).toBe(0);
      act(() => void vi.advanceTimersByTime(1));
      expect(result.current.index).toBe(1);
      act(() => void vi.advanceTimersByTime(delay));
      expect(result.current.index).toBe(2);
    },
  );

  it("playing_to_the_end_stops_at_live", () => {
    const { result } = setup();
    act(() => result.current.seek(0));
    act(() => result.current.play());
    for (let tick = 0; tick < result.current.positions.length; tick += 1) {
      act(() => void vi.advanceTimersByTime(FRAME_INTERVAL_MS));
    }
    expect(result.current.state.position).toEqual({ kind: "live" });
    expect(result.current.state.playing).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("pausing_clears_the_timer", () => {
    const { result } = setup();
    act(() => result.current.seek(0));
    act(() => result.current.play());
    expect(vi.getTimerCount()).toBe(1);
    act(() => result.current.pause());
    expect(vi.getTimerCount()).toBe(0);
    act(() => void vi.advanceTimersByTime(FRAME_INTERVAL_MS * 3));
    expect(result.current.index).toBe(0);
  });

  it("unmounting_clears_the_timer", () => {
    const { result, unmount } = setup();
    act(() => result.current.seek(0));
    act(() => result.current.play());
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("no_timer_runs_while_paused", () => {
    const { result } = setup();
    expect(vi.getTimerCount()).toBe(0);
    act(() => result.current.seek(1));
    act(() => result.current.next());
    expect(vi.getTimerCount()).toBe(0);
  });

  it("reduced_motion_read_at_each_tick_comes_from_motionMs", () => {
    const { result } = setup();
    act(() => result.current.seek(0));
    act(() => result.current.play());
    motionStub.ms = 0;
    act(() => void vi.advanceTimersByTime(FRAME_INTERVAL_MS));
    expect(result.current.state.playing).toBe(false);
    expect(result.current.index).toBe(0);
    expect(result.current.reduced).toBe(true);
    act(() => result.current.play());
    expect(result.current.state.playing).toBe(false);
  });

  it("the_replay_trace_exists_only_while_open_and_away_from_live", () => {
    const { result } = setup();
    expect(result.current.replayTrace).toBeUndefined();
    act(() => result.current.seek(0));
    expect(result.current.replayTrace?.steps.length).toBeLessThan(trace.steps.length);
    act(() => result.current.dispatch({ type: "close" }));
    expect(result.current.replayTrace).toBeUndefined();
  });
});
