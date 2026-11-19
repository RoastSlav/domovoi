import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { calls, resetApi, route } from "../../test/apiMock";
import { keys } from "../queryClient";
import {
  useAnswerStep,
  useApproveStep,
  useCancelOperation,
  useDenyStep,
  usePauseOperation,
  useResumeOperation,
} from "./operationControls";
import { OPERATION_PAGE, useOperations } from "./operations";

vi.mock("../client", () => import("../../test/apiMock"));

const SUMMARY = { items: [], next_cursor: null, as_of_seq: 0 };
const CONTROL = { as_of_seq: 1, operation: {} };

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  return { client, wrapper };
}

const body = (index: number) => calls[index]?.init.body;
const query = (index: number) => calls[index]?.init.params?.query;

beforeEach(resetApi);

describe("operation hooks", () => {
  it("list_hook_sends_filters_and_the_cursor", async () => {
    route("GET /api/v1/operations", (init) =>
      init.params?.query?.cursor === undefined ? { ...SUMMARY, next_cursor: "c2" } : SUMMARY,
    );
    const { wrapper } = setup();
    const { result } = renderHook(
      () => useOperations({ status: "paused", host: "nas", kind: undefined }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(query(0)).toEqual({ status: "paused", host: "nas", limit: OPERATION_PAGE });
    await act(async () => {
      await result.current.fetchNextPage();
    });
    expect(query(1)).toEqual({
      status: "paused",
      host: "nas",
      limit: OPERATION_PAGE,
      cursor: "c2",
    });
  });

  it("each_mutation_sends_its_path_and_body", async () => {
    for (const path of [
      "POST /api/v1/operations/{id}/steps/{step}/approve",
      "POST /api/v1/operations/{id}/steps/{step}/deny",
      "POST /api/v1/operations/{id}/cancel",
      "POST /api/v1/operations/{id}/pause",
      "POST /api/v1/operations/{id}/resume",
      "POST /api/v1/operations/{id}/steps/{step}/answer",
    ]) {
      route(path, () => CONTROL);
    }
    const { wrapper } = setup();
    const approve = renderHook(() => useApproveStep("op"), { wrapper });
    const deny = renderHook(() => useDenyStep("op"), { wrapper });
    const cancel = renderHook(() => useCancelOperation("op"), { wrapper });
    const pause = renderHook(() => usePauseOperation("op"), { wrapper });
    const resume = renderHook(() => useResumeOperation("op"), { wrapper });
    const answer = renderHook(() => useAnswerStep("op"), { wrapper });
    await act(async () => {
      await approve.result.current.mutateAsync({ step: "s1", planHash: "ab" });
      await approve.result.current.mutateAsync({ step: "s1", planHash: "ab", confirmText: "x" });
      await deny.result.current.mutateAsync({ step: "s1" });
      await cancel.result.current.mutateAsync({ reason: "no" });
      await cancel.result.current.mutateAsync({});
      await pause.result.current.mutateAsync();
      await resume.result.current.mutateAsync({ extend: [{ kind: "cycles", limit: 5 }] });
      await answer.result.current.mutateAsync({ step: "s2", text: "hi" });
    });
    expect(body(0)).toEqual({ plan_hash: "ab" });
    expect(Object.keys(body(0) as object)).not.toContain("confirm_text");
    expect(body(1)).toEqual({ plan_hash: "ab", confirm_text: "x" });
    expect(calls[0]?.init.params?.path).toEqual({ id: "op", step: "s1" });
    expect(calls[2]?.route).toBe("POST /api/v1/operations/{id}/steps/{step}/deny");
    expect(body(3)).toEqual({ reason: "no" });
    expect(body(4)).toEqual({});
    expect(body(5)).toBeUndefined();
    expect(body(6)).toEqual({ extend: [{ kind: "cycles", limit: 5 }] });
    expect(body(7)).toEqual({ text: "hi" });
    expect(calls[7]?.init.params?.path).toEqual({ id: "op", step: "s2" });
  });

  it("each_mutation_invalidates_operation_queries_overview_and_incident_details", async () => {
    route("POST /api/v1/operations/{id}/pause", () => CONTROL);
    const { client, wrapper } = setup();
    const spy = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => usePauseOperation("op"), { wrapper });
    await act(async () => {
      await result.current.mutateAsync();
    });
    const invalidated = spy.mock.calls.map(([filter]) => filter?.queryKey);
    expect(invalidated).toEqual(
      expect.arrayContaining([keys.operations, keys.overview, keys.incidentDetails]),
    );
  });
});
