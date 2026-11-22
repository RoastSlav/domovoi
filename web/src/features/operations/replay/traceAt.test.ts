import type { OperationTrace } from "../../../api/operationTrace";
import {
  TRACE_FIXTURES,
  deepFreeze,
  ev,
  historyFor,
  historyOf,
  must,
} from "../../../test/replayFixtures";
import { makeTrace } from "../../../test/syntheticTrace";
import { buildFrames, positionsOf } from "./frames";
import { traceAt } from "./traceAt";
import type { OperationHistory } from "./types";

const waiting = TRACE_FIXTURES["waiting-approval"] as OperationTrace;
const id = (n: number): string => `01900000-0000-7000-8000-00000001000${n}`;
const ms = (iso: string): number => Date.parse(iso);

function fold(final: OperationTrace, history: OperationHistory) {
  const frames = buildFrames(history.events);
  return { frames, at: (atMs: number) => traceAt({ final, history, frames, atMs }) };
}

function withoutCreated(
  history: OperationHistory,
  steps: string[],
  reason: string,
): OperationHistory {
  return historyOf(
    history.events.filter(
      (e) => !(e.kind === "operation.step_created" && e.step && steps.includes(e.step)),
    ),
    reason,
  );
}

test("a_step_does_not_exist_before_its_creation_second", () => {
  const { frames, at } = fold(waiting, historyFor(waiting));
  expect(frames).toHaveLength(13);
  // frames, by hand: 08:52:20 (none), 53:21, 53:30, 54:21, 54:30, 55:21, 55:30, 56:21, 56:22, 56:23, 56:30, 59:21, 59:30
  const counts = frames.map((f) => at(f.atMs).steps.length);
  expect(counts).toEqual([0, 1, 1, 2, 2, 3, 3, 4, 5, 6, 6, 7, 7]);
});

test("a_pending_step_with_no_timestamps_appears_pending", () => {
  const { frames, at } = fold(waiting, historyFor(waiting));
  for (const frame of frames.slice(8)) {
    const steps = at(frame.atMs).steps;
    expect(steps.find((s) => s.id === id(5))?.outcome).toBe("pending");
  }
  expect(at(must(frames[7]).atMs).steps.some((s) => s.id === id(5))).toBe(false);
});

test("a_step_becomes_final_at_the_second_of_its_ended_at", () => {
  const { at } = fold(waiting, historyFor(waiting));
  const before = at(ms("2025-10-09T08:53:29Z")).steps.find((s) => s.id === id(1));
  const during = at(ms("2025-10-09T08:53:21Z")).steps.find((s) => s.id === id(1));
  const after = at(ms("2025-10-09T08:53:30Z")).steps.find((s) => s.id === id(1));
  expect(before?.outcome).toBe("running");
  expect(before?.ended_at).toBeNull();
  expect(during?.ended_at).toBeNull();
  expect(after?.outcome).toBe("succeeded");
  expect(after?.ended_at).toBe("2025-10-09T08:53:30Z");
});

test("a_step_is_running_between_started_at_and_ended_at", () => {
  const trace: OperationTrace = {
    ...waiting,
    steps: waiting.steps.map((s) =>
      s.id === id(2) ? { ...s, started_at: "2025-10-09T08:54:25Z" } : s,
    ),
  };
  const history = historyOf([
    ev(1, "2025-10-09T08:52:20Z", "operation.created"),
    ...trace.steps.map((s, i) => ev(2 + i, "2025-10-09T08:52:21Z", "operation.step_created", s.id)),
    ev(20, "2025-10-09T08:54:30Z", "operation.step_finished", id(2)),
  ]);
  const { at } = fold(trace, history);
  const outcome = (iso: string, step = id(2)) =>
    at(ms(iso)).steps.find((s) => s.id === step)?.outcome;
  expect(outcome("2025-10-09T08:54:24Z")).toBe("pending");
  expect(outcome("2025-10-09T08:54:25Z")).toBe("running");
  expect(outcome("2025-10-09T08:54:30Z")).toBe("succeeded");
  // a step whose final outcome is pending is never running, even with a start time
  const pendingWithStart: OperationTrace = {
    ...waiting,
    steps: waiting.steps.map((s) =>
      s.id === id(5) ? { ...s, started_at: "2025-10-09T08:54:25Z" } : s,
    ),
  };
  const fold2 = fold(pendingWithStart, history);
  const seen = fold2.frames.map((f) => fold2.at(f.atMs).steps.find((s) => s.id === id(5))?.outcome);
  expect(seen).not.toContain("running");
});

test("a_dispatch_row_makes_an_execution_running_when_it_has_no_started_at", () => {
  const trace = makeTrace(7, 1); // s6 is an execution, running, with no started_at
  const history = historyOf([
    ev(1, "2026-01-01T00:00:00Z", "operation.created"),
    ...trace.steps.map((s, i) =>
      ev(2 + i, `2026-01-01T00:00:0${i + 1}Z`, "operation.step_created", s.id),
    ),
    ev(20, "2026-01-01T00:00:10Z", "operation.execution", "s6", "awaiting_approval"),
    ev(21, "2026-01-01T00:00:12Z", "operation.execution", "s6", "dispatched"),
  ]);
  const { at } = fold(trace, history);
  const outcome = (s: string) => at(ms(s)).steps.find((x) => x.id === "s6")?.outcome;
  expect(outcome("2026-01-01T00:00:11Z")).toBe("pending");
  expect(outcome("2026-01-01T00:00:12Z")).toBe("running");
});

test("a_final_outcome_without_an_ended_at_shows_only_at_live", () => {
  const verified = TRACE_FIXTURES["verified-success"] as OperationTrace;
  const trace: OperationTrace = {
    ...verified,
    steps: verified.steps.map((s, i) => (i === 0 ? { ...s, ended_at: null } : s)),
  };
  const { frames, at } = fold(trace, historyFor(verified));
  for (const frame of frames) {
    const first = at(frame.atMs).steps[0];
    if (first) {
      expect(first.outcome).not.toBe("succeeded");
    }
  }
});

test("edges_appear_with_both_their_steps", () => {
  const { frames, at } = fold(waiting, historyFor(waiting));
  for (const frame of frames) {
    const shown = at(frame.atMs);
    const ids = new Set(shown.steps.map((s) => s.id));
    expect(shown.edges.every((e) => ids.has(e.from) && ids.has(e.to))).toBe(true);
    const expected = waiting.edges.filter((e) => ids.has(e.from) && ids.has(e.to));
    expect(shown.edges).toEqual(expected);
  }
});

test("the_fold_does_not_mutate_the_final_trace", () => {
  const final = deepFreeze(structuredClone(waiting));
  const history = deepFreeze(historyFor(waiting));
  const frames = deepFreeze(buildFrames(history.events));
  for (const frame of frames) {
    expect(() => traceAt({ final, history, frames, atMs: frame.atMs })).not.toThrow();
  }
  expect(final).toEqual(waiting);
});

test.each(Object.keys(TRACE_FIXTURES))(
  "the_last_frame_of_a_complete_history_reproduces_the_final_trace_for_%s",
  (name) => {
    const final = TRACE_FIXTURES[name] as OperationTrace;
    const history = historyFor(final);
    const { frames, at } = fold(final, history);
    const last = at(must(frames[frames.length - 1]).atMs);
    expect(last.steps).toEqual(final.steps);
    expect(last.edges).toEqual(final.edges);
    expect(last.operation.status).toBe(final.operation.status);
    // a complete history ends at live, so the slider's last position is the live trace itself
    expect(positionsOf(frames, true).at(-1)).toEqual({ kind: "live" });
  },
);

test.each([
  [12, 3],
  [60, 4],
  [150, 5],
])("existence_is_monotone_and_edges_join_existing_steps_for_%i_steps_seed_%i", (n, seed) => {
  const final = makeTrace(n, seed);
  const history = historyOf([
    ev(1, "2026-01-01T00:00:00Z", "operation.created"),
    ...final.steps.map((s, i) =>
      ev(
        2 + i,
        new Date(Date.UTC(2026, 0, 1, 0, 0, 1 + Math.floor(i / 2))).toISOString(),
        "operation.step_created",
        s.id,
      ),
    ),
  ]);
  const { frames, at } = fold(final, history);
  let previous = new Set<string>();
  for (const frame of frames) {
    const shown = at(frame.atMs);
    const ids = new Set(shown.steps.map((s) => s.id));
    for (const old of previous) {
      expect(ids.has(old)).toBe(true);
    }
    expect(shown.edges.every((e) => ids.has(e.from) && ids.has(e.to))).toBe(true);
    previous = ids;
  }
  expect(previous.size).toBe(n);
});

test("a_pruned_head_makes_older_steps_exist_from_the_start", () => {
  const history = withoutCreated(historyFor(waiting), [id(1), id(2)], "pruned");
  const { frames, at } = fold(waiting, history);
  const first = at(must(frames[0]).atMs);
  expect(first.steps.map((s) => s.id)).toEqual([id(1), id(2)]);
  expect(first.steps.every((s) => s.outcome === "pending")).toBe(true);
  expect(at(ms("2025-10-09T08:55:20Z")).steps.some((s) => s.id === id(3))).toBe(false);
  expect(at(ms("2025-10-09T08:55:21Z")).steps.map((s) => s.id)).toEqual([id(1), id(2), id(3)]);
});

test("a_capped_tail_hides_newer_steps_until_live", () => {
  const history = withoutCreated(historyFor(waiting), [id(6), id(7)], "capped");
  const { frames, at } = fold(waiting, history);
  for (const frame of frames) {
    const ids = at(frame.atMs).steps.map((s) => s.id);
    expect(ids).not.toContain(id(6));
    expect(ids).not.toContain(id(7));
  }
  expect(at(must(frames[frames.length - 1]).atMs).steps).toHaveLength(5);
});

test("both_gaps_use_the_ordinal_rule", () => {
  const history = withoutCreated(historyFor(waiting), [id(1), id(2), id(6), id(7)], "both");
  const { frames, at } = fold(waiting, history);
  expect(at(must(frames[0]).atMs).steps.map((s) => s.id)).toEqual([id(1), id(2)]);
  const counts = frames.map((f) => at(f.atMs).steps.length);
  expect(Math.max(...counts)).toBe(5);
});

test("no_created_rows_at_all_follows_the_reason", () => {
  const all = waiting.steps.map((s) => s.id);
  const pruned = fold(waiting, withoutCreated(historyFor(waiting), all, "pruned"));
  expect(pruned.at(must(pruned.frames[0]).atMs).steps).toHaveLength(7);
  const capped = fold(waiting, withoutCreated(historyFor(waiting), all, "capped"));
  for (const frame of capped.frames) {
    expect(capped.at(frame.atMs).steps).toHaveLength(0);
  }
});

test("operation_status_follows_the_status_rows", () => {
  const history = historyOf([
    ev(1, "2026-01-01T00:00:00Z", "operation.created"),
    ev(2, "2026-01-01T00:00:03Z", "operation.status", null, "running"),
    ev(3, "2026-01-01T00:00:06Z", "operation.status", null, "paused"),
  ]);
  const final = { ...waiting, operation: { ...waiting.operation, status: "paused" } };
  const { at } = fold(final, history);
  expect(at(ms("2026-01-01T00:00:00Z")).operation.status).toBe("queued");
  expect(at(ms("2026-01-01T00:00:03Z")).operation.status).toBe("running");
  expect(at(ms("2026-01-01T00:00:06Z")).operation.status).toBe("paused");
});

test("as_of_seq_is_the_highest_sequence_at_or_before_the_moment", () => {
  const { frames, at } = fold(waiting, historyFor(waiting));
  expect(at(must(frames[0]).atMs).as_of_seq).toBe(must(frames[0]).seq);
  expect(at(must(frames[12]).atMs).as_of_seq).toBe(must(frames[12]).seq);
});

test("unknown_kinds_states_and_step_ids_do_not_throw", () => {
  const base = historyFor(waiting);
  const history = historyOf([
    ...base.events,
    ev(900, "2025-10-09T08:57:00Z", "operation.future_thing", id(2), "weird"),
    ev(901, "2025-10-09T08:57:00Z", "operation.step_created", "ghost"),
    ev(902, "2025-10-09T08:57:00Z", "operation.execution", "ghost", "never_heard_of_it"),
  ]);
  const { frames, at } = fold(waiting, history);
  const known = new Set(waiting.steps.map((s) => s.id));
  for (const frame of frames) {
    expect(at(frame.atMs).steps.every((s) => known.has(s.id))).toBe(true);
  }
});
