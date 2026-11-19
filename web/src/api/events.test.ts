import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { FakeEventSource } from "../test/fakeEventSource";
import { EVENTS_URL, MAX_RETRY_MS, PAUSED_AFTER_MS, retryDelay, useLiveEvents } from "./events";

function setup() {
  const client = new QueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  const hook = renderHook(() => useLiveEvents(), { wrapper });
  const source = FakeEventSource.all.at(-1);
  if (!source) {
    throw new Error("no EventSource opened");
  }
  return { hook, invalidate, source };
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

test("topics_are_sent_comma_joined", () => {
  const { hook, source } = setup();
  expect(source.url).toBe(`${EVENTS_URL}?topics=incidents,approvals,operations`);
  expect(source.url).toBe("/api/v1/events?topics=incidents,approvals,operations");
  hook.unmount();
});

test("live_event_invalidates_incident_queries", () => {
  const { hook, invalidate, source } = setup();
  expect(hook.result.current.status).toBe("connecting");

  act(() => source.emit("open"));
  expect(hook.result.current.status).toBe("live");

  act(() =>
    source.emit("journal", {
      seq: 1,
      aggregate_type: "incident",
      aggregate_id: "inc-1",
      kind: "incident.timeline",
      payload_v: 1,
      payload: { kind: "human.action" },
      actor: "system:incidents",
      at: "2026-11-18T10:00:00Z",
      topics: ["incidents"],
    }),
  );
  expect(invalidatedKeys(invalidate)).toEqual([
    ["incidents", "list"],
    ["incidents", "detail", "inc-1"],
    ["overview"],
  ]);

  invalidate.mockClear();
  act(() =>
    source.emit("journal", {
      seq: 2,
      aggregate_type: "action",
      aggregate_id: "act-1",
      kind: "action.transition",
      payload_v: 1,
      payload: { incident: "inc-1", from: "proposed", to: "approved" },
      actor: "system:actions",
      at: "2026-11-18T10:00:00Z",
      topics: ["approvals"],
    }),
  );
  expect(invalidatedKeys(invalidate)).toContainEqual(["actions"]);

  hook.unmount();
  expect(source.closed).toBe(true);
});

test("live_events_reconnect_with_backoff", () => {
  vi.useFakeTimers();
  const { hook, source } = setup();
  act(() => source.emit("error"));
  expect(hook.result.current.status).toBe("offline");
  expect(source.closed).toBe(true);

  act(() => vi.advanceTimersByTime(retryDelay(0) - 1));
  expect(FakeEventSource.all).toHaveLength(1);
  act(() => vi.advanceTimersByTime(1));
  expect(FakeEventSource.all).toHaveLength(2);
  expect(hook.result.current.status).toBe("connecting");

  expect(retryDelay(1)).toBe(2_000);
  expect(retryDelay(10)).toBe(MAX_RETRY_MS);
  hook.unmount();
});

test("live_updates_read_as_paused_after_five_seconds_down_and_recover_on_reconnect", () => {
  vi.useFakeTimers();
  const { hook, source } = setup();
  act(() => source.emit("open"));

  act(() => source.emit("error"));
  expect(hook.result.current.paused).toBe(false);
  // Reconnect attempts that fail in the meantime do not restart the wait.
  act(() => vi.advanceTimersByTime(retryDelay(0)));
  act(() => FakeEventSource.all.at(-1)?.emit("error"));
  act(() => vi.advanceTimersByTime(PAUSED_AFTER_MS - retryDelay(0) - 1));
  expect(hook.result.current.paused).toBe(false);
  act(() => vi.advanceTimersByTime(1));
  expect(hook.result.current.paused).toBe(true);

  act(() => vi.advanceTimersByTime(retryDelay(1)));
  act(() => FakeEventSource.all.at(-1)?.emit("open"));
  expect(hook.result.current).toEqual({ status: "live", paused: false });
  hook.unmount();
});

test("a_short_outage_never_reads_as_paused", () => {
  vi.useFakeTimers();
  const { hook, source } = setup();
  act(() => source.emit("error"));
  act(() => vi.advanceTimersByTime(retryDelay(0)));
  act(() => FakeEventSource.all.at(-1)?.emit("open"));

  act(() => vi.advanceTimersByTime(PAUSED_AFTER_MS * 2));
  expect(hook.result.current.paused).toBe(false);
  hook.unmount();
});
