import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import searchGlobal from "../../test/fixtures/experience/search-global.json";
import searchHost from "../../test/fixtures/experience/search-host.json";
import similar from "../../test/fixtures/experience/similar.json";
import drafting from "../../test/fixtures/experience/summary-drafting.json";
import ready from "../../test/fixtures/experience/summary-ready.json";
import { applyJournal, type JournalFrame } from "../journalFrames";
import { keys } from "../queryClient";
import type { ExperienceSearch, ResolutionSummary, SimilarOperations } from "../types";
import {
  useEditSummaryField,
  useExperienceSearch,
  useResolutionSummary,
  useSimilarOperations,
} from "./experience";

vi.mock("../client", () => import("../../test/apiMock"));

const LABEL = "historical evidence, not authorization";
const OP = "0190a0a0-0000-7000-8000-0000000000b1";

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  return { client, wrapper };
}

beforeEach(resetApi);

describe("experience hooks", () => {
  it("the_hooks_call_the_documented_paths_and_are_disabled_without_input", async () => {
    route("GET /api/v1/operations/{id}/summary", () => ready);
    route("GET /api/v1/operations/{id}/similar", () => similar);
    route("GET /api/v1/incidents/{id}/similar-operations", () => similar);
    route("GET /api/v1/experience/search", () => searchHost);
    const { wrapper } = setup();

    const idle = renderHook(
      () => [
        useResolutionSummary(undefined),
        useSimilarOperations(undefined),
        useExperienceSearch(undefined),
      ],
      { wrapper },
    );
    expect(idle.result.current.every((q) => q.fetchStatus === "idle")).toBe(true);
    expect(calls).toHaveLength(0);

    const summary = renderHook(() => useResolutionSummary(OP), { wrapper });
    const byOperation = renderHook(() => useSimilarOperations({ operation: OP }, 7), { wrapper });
    const byIncident = renderHook(() => useSimilarOperations({ incident: "inc" }), { wrapper });
    const search = renderHook(
      () => useExperienceSearch({ q: "plex", scope: "host", host: "nas" }),
      { wrapper },
    );
    const global = renderHook(() => useExperienceSearch({ q: "plex", scope: "global" }), {
      wrapper,
    });
    await waitFor(() => {
      for (const hook of [summary, byOperation, byIncident, search, global]) {
        expect(hook.result.current.isSuccess).toBe(true);
      }
    });
    const seen = (path: string) => calls.find((c) => c.route === path)?.init;
    expect(seen("GET /api/v1/operations/{id}/summary")?.params?.path).toEqual({ id: OP });
    expect(seen("GET /api/v1/operations/{id}/similar")?.params).toEqual({
      path: { id: OP },
      query: { limit: 7 },
    });
    expect(seen("GET /api/v1/incidents/{id}/similar-operations")?.params?.path).toEqual({
      id: "inc",
    });
    const queries = calls
      .filter((c) => c.route === "GET /api/v1/experience/search")
      .map((c) => c.init.params?.query);
    expect(queries).toContainEqual({ q: "plex", scope: "host", host: "nas" });
    expect(queries).toContainEqual({ q: "plex", scope: "global" });
  });

  it("each_fixture_has_the_generated_type_and_the_label", () => {
    const summaries: ResolutionSummary[] = [ready, drafting];
    const lists: (SimilarOperations | ExperienceSearch)[] = [similar, searchHost, searchGlobal];
    for (const item of [...summaries, ...lists]) {
      expect(item.label).toBe(LABEL);
    }
    expect(ready.fields.map((f) => f.field)).toEqual([
      "outcome",
      "verified",
      "subjects",
      "contracts_executed",
      "hosts",
      "problem",
      "cause",
      "fix",
    ]);
    expect(drafting.state).toBe("drafting");
  });

  it("a_frame_for_operation_summary_refreshes_the_summary_and_similar_queries", async () => {
    const { client } = setup();
    const stale = [keys.operationSummary(OP), keys.operationSimilar(OP, 5)];
    for (const key of stale) {
      client.setQueryData(key, { as_of_seq: 1 });
    }
    const frame: JournalFrame = {
      seq: 9,
      aggregate_type: "operation",
      aggregate_id: OP,
      kind: "operation.summary",
      payload_v: 1,
      payload: { operation: OP, field: "cause", revision: 2, source: "user" },
      actor: "user:0190a0a0-0000-7000-8000-0000000000d1",
      at: "2026-10-31T12:07:00Z",
      topics: ["operations", `operation:${OP}`],
    };
    await applyJournal(client, frame);
    for (const key of stale) {
      expect(client.getQueryState(key)?.isInvalidated, key.join("/")).toBe(true);
    }
  });

  it("the_edit_mutation_invalidates_the_operation_prefix_and_does_not_retry", async () => {
    route("POST /api/v1/operations/{id}/summary/{field}", () => ({
      field: ready.fields[6],
      unchanged: false,
    }));
    const { client, wrapper } = setup();
    client.setQueryData(keys.operationSummary(OP), { as_of_seq: 1 });
    const { result } = renderHook(() => useEditSummaryField(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({
        operation: OP,
        field: "cause",
        value: "the disk was full",
        expectedRevision: 2,
      });
    });
    expect(calls[0]?.init.params?.path).toEqual({ id: OP, field: "cause" });
    expect(calls[0]?.init.body).toEqual({
      value: "the disk was full",
      expected_revision: 2,
    });
    expect(client.getQueryState(keys.operationSummary(OP))?.isInvalidated).toBe(true);

    route("POST /api/v1/operations/{id}/summary/{field}", () =>
      failure(409, "conflict", "the summary changed since you opened it; reload and try again"),
    );
    await act(async () => {
      await result.current
        .mutateAsync({ operation: OP, field: "cause", value: "x", expectedRevision: 1 })
        .catch(() => undefined);
    });
    expect(calls.filter((c) => c.route.startsWith("POST"))).toHaveLength(2);
  });
});
