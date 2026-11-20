import { NO_LIVE, liveReducer, type LiveState } from "./operatorLive";

const at = (turn: string, attempt = 1, text = "hi"): LiveState => ({
  turn,
  attempt,
  text,
  tools: [],
});
const frame = (name: string, data: unknown) => ({ name, data });

test("a_late_frame_for_another_turn_is_ignored", () => {
  const state = at("B");
  expect(liveReducer(state, frame("live.text", { turn: "A", attempt: 5, text: "old" }), "B")).toBe(
    state,
  );
});

test("a_higher_attempt_replaces_the_text_and_a_lower_one_is_ignored", () => {
  let state = liveReducer(NO_LIVE, frame("live.start", { turn: "T" }), "T");
  state = liveReducer(state, frame("live.text", { turn: "T", attempt: 1, text: "one" }), "T");
  state = liveReducer(state, frame("live.text", { turn: "T", attempt: 2, text: "two" }), "T");
  expect(state.text).toBe("two");
  state = liveReducer(state, frame("live.text", { turn: "T", attempt: 1, text: "one more" }), "T");
  expect(state).toMatchObject({ attempt: 2, text: "two" });
});

test("live_end_clears_only_its_own_turn", () => {
  const state = at("T");
  expect(liveReducer(state, frame("live.end", { turn: "X", status: "complete" }), "T")).toBe(state);
  expect(liveReducer(state, frame("live.end", { turn: "T", status: "complete" }), "T")).toBe(
    NO_LIVE,
  );
});

test("a_reload_mid_turn_adopts_the_active_turn", () => {
  const f = frame("live.text", { turn: "T", attempt: 3, text: "so far" });
  expect(liveReducer(NO_LIVE, f, "T")).toMatchObject({ turn: "T", text: "so far" });
  expect(liveReducer(NO_LIVE, f, "other")).toBe(NO_LIVE);
});

test("malformed_or_unknown_frames_are_ignored", () => {
  const state = at("T");
  for (const f of [
    frame("live.text", null),
    frame("live.text", "x"),
    frame("live.text", { turn: "T", attempt: "2", text: 1 }),
    frame("live.mystery", { turn: "T" }),
    frame("live.tool", { turn: "T", tool: 4 }),
  ]) {
    expect(liveReducer(state, f, "T")).toEqual(state);
  }
});

test("tools_start_and_finish", () => {
  let state = at("T");
  state = liveReducer(
    state,
    frame("live.tool", { turn: "T", tool: "journal", state: "started", failed: false }),
    "T",
  );
  state = liveReducer(
    state,
    frame("live.tool", { turn: "T", tool: "journal", state: "finished", failed: true }),
    "T",
  );
  expect(state.tools).toEqual([{ tool: "journal", state: "finished", failed: true }]);
});
