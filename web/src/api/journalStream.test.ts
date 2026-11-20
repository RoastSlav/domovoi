import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { FakeEventSource } from "../test/fakeEventSource";
import { retryDelay } from "./events";
import type { JournalFrame } from "./journalFrames";
import { useJournalStream } from "./journalStream";

function frame(seq: number): JournalFrame {
  return {
    seq,
    aggregate_type: "incident",
    aggregate_id: "inc-1",
    kind: "incident.timeline",
    payload_v: 1,
    payload: null,
    actor: "system:incidents",
    at: "2026-11-18T10:00:00Z",
    topics: ["incidents"],
  };
}

function setup(seed?: (client: QueryClient) => void) {
  const client = new QueryClient();
  seed?.(client);
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const onFrame = vi.fn();
  const onReset = vi.fn();
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  const hook = renderHook(
    () => useJournalStream({ topics: ["incidents", "approvals"], onFrame, onReset }),
    { wrapper },
  );
  const current = () => {
    const source = FakeEventSource.all.at(-1);
    if (!source) {
      throw new Error("no EventSource opened");
    }
    return source;
  };
  return { hook, invalidate, onFrame, onReset, current, client };
}

function invalidatedKeys(invalidate: { mock: { calls: unknown[][] } }): unknown[] {
  return invalidate.mock.calls.map((args) => (args[0] as { queryKey: unknown }).queryKey);
}

beforeEach(() => {
  FakeEventSource.all = [];
  vi.stubGlobal("EventSource", FakeEventSource);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("a_reconnect_resumes_from_the_last_seq", () => {
  vi.useFakeTimers();
  const { hook, current } = setup();
  expect(FakeEventSource.urls()).toEqual(["/api/v1/events?topics=incidents,approvals"]);
  act(() => current().emit("journal", frame(5)));
  act(() => current().emit("journal", frame(6)));
  act(() => current().emit("error"));
  act(() => vi.advanceTimersByTime(retryDelay(0)));
  expect(FakeEventSource.all.at(-1)?.url).toBe("/api/v1/events?topics=incidents,approvals&after=6");
  hook.unmount();
});

test("frames_at_or_below_the_last_seq_are_ignored", () => {
  const { hook, onFrame, current } = setup();
  act(() => current().emit("journal", frame(7)));
  act(() => current().emit("journal", frame(7)));
  act(() => current().emit("journal", frame(6)));
  expect(onFrame).toHaveBeenCalledTimes(1);
  expect(onFrame).toHaveBeenCalledWith(expect.objectContaining({ seq: 7 }));
  hook.unmount();
});

test("malformed_frames_are_ignored", () => {
  const { hook, onFrame, current } = setup();
  act(() => current().emit("journal", "not json"));
  act(() => current().emit("journal", { seq: "x" }));
  expect(onFrame).not.toHaveBeenCalled();
  hook.unmount();
});

test("reset_adopts_the_head_and_refetches_the_live_queries", () => {
  vi.useFakeTimers();
  const { hook, invalidate, onReset, current } = setup();
  act(() => current().emit("reset", { reason: "pruned", head_seq: 40 }));
  expect(invalidatedKeys(invalidate)).toEqual([
    ["incidents"],
    ["actions"],
    ["overview"],
    ["operations"],
  ]);
  expect(onReset).toHaveBeenCalledTimes(1);

  act(() => current().emit("error"));
  act(() => vi.advanceTimersByTime(retryDelay(0)));
  expect(FakeEventSource.all.at(-1)?.url).toContain("after=40");
  hook.unmount();
});

test("a_fresh_ready_refetches_only_snapshots_older_than_the_head", () => {
  const { hook, onReset, current, client } = setup((c) => {
    c.setQueryData(["incidents", "list", { a: 1 }], { as_of_seq: 10 });
    c.setQueryData(["incidents", "list", { a: 2 }], { as_of_seq: 50 });
  });
  act(() =>
    current().emit("ready", { head_seq: 40, resumed: false, topics: ["incidents", "approvals"] }),
  );
  expect(client.getQueryState(["incidents", "list", { a: 1 }])?.isInvalidated).toBe(true);
  expect(client.getQueryState(["incidents", "list", { a: 2 }])?.isInvalidated).toBe(false);
  expect(onReset).toHaveBeenCalledTimes(1);
  hook.unmount();
});

test("a_resumed_ready_refetches_nothing", () => {
  const { hook, invalidate, onReset, current } = setup();
  act(() => current().emit("ready", { head_seq: 40, resumed: true, topics: ["incidents"] }));
  expect(invalidate).not.toHaveBeenCalled();
  expect(onReset).not.toHaveBeenCalled();
  hook.unmount();
});

test("onLive_receives_only_live_frames_and_never_moves_the_cursor", () => {
  vi.useFakeTimers();
  const client = new QueryClient();
  const onLive = vi.fn();
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  renderHook(() => useJournalStream({ topics: ["conversations:c1"], onFrame: vi.fn(), onLive }), {
    wrapper,
  });
  const source = FakeEventSource.all.at(-1);
  source?.emit("live.text", { turn: "T", attempt: 1, text: "hi" });
  source?.emit("live.text", "not json {");
  source?.emit("journal", frame(7));
  expect(onLive).toHaveBeenCalledTimes(1);
  expect(onLive).toHaveBeenCalledWith("live.text", { turn: "T", attempt: 1, text: "hi" });
  source?.emit("error");
  act(() => {
    vi.advanceTimersByTime(retryDelay(0));
  });
  expect(FakeEventSource.all.at(-1)?.url).toContain("after=7");
});
