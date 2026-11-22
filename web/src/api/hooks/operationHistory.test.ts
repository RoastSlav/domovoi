import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { applyJournal, type JournalFrame } from "../journalFrames";
import { calls, resetApi, route } from "../../test/apiMock";
import { historyOf } from "../../test/replayFixtures";
import { keys } from "../queryClient";
import { useOperationHistory } from "./operationHistory";

vi.mock("../client", () => import("../../test/apiMock"));

const ID = "0190a0a0-0000-7000-8000-0000000000b1";

function setup() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  return { client, wrapper };
}

beforeEach(resetApi);

describe("the operation history hook", () => {
  it("history_hook_requests_the_path_and_sits_under_the_operation_key", async () => {
    route("GET /api/v1/operations/{id}/history", () => historyOf([]));
    const { client, wrapper } = setup();
    const { result } = renderHook(() => useOperationHistory(ID, true), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls[0]?.route).toBe("GET /api/v1/operations/{id}/history");
    expect(calls[0]?.init.params?.path).toEqual({ id: ID });
    expect(keys.operationHistory(ID).slice(0, 3)).toEqual(keys.operation(ID));
    expect(client.getQueryData(keys.operationHistory(ID))).toBeDefined();
  });

  it("history_hook_is_disabled_until_enabled", async () => {
    route("GET /api/v1/operations/{id}/history", () => historyOf([]));
    const { wrapper } = setup();
    const { result, rerender } = renderHook(
      ({ on, id }: { on: boolean; id: string | undefined }) => useOperationHistory(id, on),
      { wrapper, initialProps: { on: false, id: ID as string | undefined } },
    );
    expect(result.current.fetchStatus).toBe("idle");
    rerender({ on: true, id: undefined });
    expect(result.current.fetchStatus).toBe("idle");
    expect(calls).toHaveLength(0);
    rerender({ on: true, id: ID });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(calls).toHaveLength(1);
  });

  it("a_journal_frame_for_the_operation_invalidates_the_history", async () => {
    route("GET /api/v1/operations/{id}/history", () => ({ ...historyOf([]), as_of_seq: 5 }));
    const { client, wrapper } = setup();
    const { result } = renderHook(() => useOperationHistory(ID, true), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const frame = (seq: number): JournalFrame => ({
      seq,
      aggregate_type: "operation",
      aggregate_id: ID,
      kind: "operation.step_created",
      payload_v: 1,
      payload: null,
      actor: "",
      at: "2026-01-01T00:00:00Z",
      topics: [],
    });
    await applyJournal(client, frame(5));
    expect(calls).toHaveLength(1);
    await applyJournal(client, frame(6));
    await waitFor(() => expect(calls.length).toBeGreaterThan(1));
  });
});
