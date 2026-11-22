import type { OperationTrace } from "../../../api/operationTrace";
import { ev, must } from "../../../test/replayFixtures";
import { makeTrace } from "../../../test/syntheticTrace";
import { buildFrames, floorSecond, frameCaption, positionsOf } from "./frames";

const A = "2026-01-01T00:00:00Z";

test("floor_second_drops_fractions_and_rejects_garbage", () => {
  expect(floorSecond("2026-01-01T00:00:01.900Z")).toBe(Date.parse("2026-01-01T00:00:01Z"));
  expect(floorSecond("not a time")).toBeNull();
});

test("frames_are_whole_seconds_in_ascending_order", () => {
  const frames = buildFrames([
    ev(3, "2026-01-01T00:00:02.100Z", "operation.step_created", "b"),
    ev(1, "2026-01-01T00:00:00.200Z", "operation.created"),
    ev(2, "2026-01-01T00:00:00.900Z", "operation.step_created", "a"),
  ]);
  expect(frames.map((f) => f.atMs)).toEqual([Date.parse(A), Date.parse("2026-01-01T00:00:02Z")]);
  expect(frames[0]?.events.map((e) => e.seq)).toEqual([1, 2]);
  expect(frames.map((f) => f.seq)).toEqual([2, 3]);
});

test("an_unparsable_time_is_ignored", () => {
  const good = [ev(1, A, "operation.created"), ev(3, "2026-01-01T00:00:01Z", "operation.status")];
  const frames = buildFrames([must(good[0]), ev(2, "garbage", "operation.status"), must(good[1])]);
  expect(frames.map((f) => f.seq)).toEqual([1, 3]);
  expect(buildFrames([ev(1, "x", "operation.created")])).toEqual([]);
});

test("clock_skew_orders_frames_by_time_and_keeps_seq", () => {
  const frames = buildFrames([
    ev(1, "2026-01-01T00:00:05Z", "a"),
    ev(2, "2026-01-01T00:00:01Z", "b"),
  ]);
  expect(frames.map((f) => f.seq)).toEqual([2, 1]);
});

test("positions_replace_the_last_frame_with_live_when_the_history_reaches_the_present", () => {
  const frames = buildFrames([
    ev(1, A, "k"),
    ev(2, "2026-01-01T00:00:01Z", "k"),
    ev(3, "2026-01-01T00:00:02Z", "k"),
  ]);
  const [f0, f1, f2] = frames.map((f) => ({ kind: "frame" as const, atMs: f.atMs }));
  expect(positionsOf(frames, true)).toEqual([f0, f1, { kind: "live" }]);
  expect(positionsOf(frames, false)).toEqual([f0, f1, f2, { kind: "live" }]);
  expect(positionsOf([], true)).toEqual([{ kind: "live" }]);
  expect(positionsOf([], false)).toEqual([{ kind: "live" }]);
});

function traceOfKinds(): OperationTrace {
  const t = makeTrace(8, 1);
  return t; // kinds cycle: evidence_request, reasoning_turn, assessment, plan, policy_decision, approval, execution, verification
}

test("frame_caption_uses_the_vocabulary_words_and_caps_at_three_parts", () => {
  const trace = makeTrace(9, 1); // s0 evidence_request, s8 evidence_request, s3 plan
  const frame = (events: ReturnType<typeof ev>[]) => must(buildFrames(events)[0]);
  expect(
    frameCaption(
      frame([
        ev(1, A, "operation.step_created", "s0"),
        ev(2, A, "operation.step_created", "s8"),
        ev(3, A, "operation.step_created", "s3"),
      ]),
      trace,
    ),
  ).toBe("Evidence ×2 added; Plan added");
  const four = frameCaption(
    frame([
      ev(1, A, "operation.step_created", "s0"),
      ev(2, A, "operation.step_created", "s3"),
      ev(3, A, "operation.execution", "s6", "dispatched"),
      ev(4, A, "operation.status", null, "paused"),
    ]),
    trace,
  );
  expect(four).toMatch(/^Evidence added; Plan added; Action: .+; and 1 more$/);
  expect(frameCaption({ atMs: 0, seq: 0, events: [] }, trace)).toBe("Recorded");
  expect(
    frameCaption(
      frame([ev(1, A, "operation.future_thing"), ev(2, A, "operation.step_started", "s0")]),
      trace,
    ),
  ).toBe("2 other changes");
});

test("frame_caption_tolerates_unknown_steps_and_states", () => {
  const trace = traceOfKinds();
  const caption = frameCaption(
    must(
      buildFrames([
        ev(1, A, "operation.step_created", "missing"),
        ev(2, A, "operation.execution", "s6", "brand_new_state"),
      ])[0],
    ),
    trace,
  );
  expect(caption).toMatch(/^step added; Action: /);
});
