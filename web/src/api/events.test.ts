import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { EVENTS_URL, MAX_RETRY_MS, retryDelay, useLiveEvents } from "./events";

type Listener = (event: MessageEvent) => void;

class FakeEventSource {
  static all: FakeEventSource[] = [];
  readonly url: string;
  closed = false;
  private listeners = new Map<string, Listener[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.all.push(this);
  }

  addEventListener(type: string, listener: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close() {
    this.closed = true;
  }

  emit(type: string, data?: unknown) {
    const event = new MessageEvent(type, { data: data === undefined ? "" : JSON.stringify(data) });
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

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

test("live_event_invalidates_incident_queries", () => {
  const { hook, invalidate, source } = setup();
  expect(source.url).toBe(EVENTS_URL);
  expect(hook.result.current).toBe("connecting");

  act(() => source.emit("open"));
  expect(hook.result.current).toBe("live");

  act(() =>
    source.emit("incident", {
      kind: "incident",
      event: "incident_acked",
      incident_id: "inc-1",
      status: "acked",
    }),
  );
  expect(invalidatedKeys(invalidate)).toEqual([
    ["incidents", "list"],
    ["incidents", "detail", "inc-1"],
  ]);

  invalidate.mockClear();
  act(() =>
    source.emit("action", {
      kind: "action",
      event: "action_changed",
      incident_id: "inc-1",
      action_id: "act-1",
      status: "approved",
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
  expect(hook.result.current).toBe("offline");
  expect(source.closed).toBe(true);

  act(() => vi.advanceTimersByTime(retryDelay(0) - 1));
  expect(FakeEventSource.all).toHaveLength(1);
  act(() => vi.advanceTimersByTime(1));
  expect(FakeEventSource.all).toHaveLength(2);
  expect(hook.result.current).toBe("connecting");

  expect(retryDelay(1)).toBe(2_000);
  expect(retryDelay(10)).toBe(MAX_RETRY_MS);
  hook.unmount();
});
