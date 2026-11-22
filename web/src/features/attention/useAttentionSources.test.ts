import { describe, expect, it, vi } from "vitest";
import type { Action, Host, IncidentSummary, OperationSummary } from "../../api/types";
import { summary } from "../../test/attentionFixtures";
import { nas } from "../../test/fixtures";
import {
  summarizeSources,
  type SourceName,
  type SourcePart,
  type SourcesParts,
} from "./useAttentionSources";

vi.mock("../../api/client", () => import("../../test/apiMock"));

/** A source that answered with `rows` (`null`: not answered yet); `over` turns it into a failure or a partial list. */
function part<T>(
  name: SourceName,
  rows: readonly T[] | null = [],
  over: Partial<SourcePart<T>> = {},
): SourcePart<T> {
  return {
    name,
    rows: rows ?? undefined,
    isError: false,
    error: null,
    isPending: rows === null,
    hasMore: false,
    retry: () => undefined,
    ...over,
  };
}

function parts(over: Partial<SourcesParts> = {}): SourcesParts {
  return {
    incidents: part<IncidentSummary>("incidents"),
    proposed: part<Action>("approvals"),
    unknownActions: part<Action>("unknown results"),
    waitingApproval: part<OperationSummary>("operations waiting for approval"),
    waitingUser: part<OperationSummary>("operations waiting for an answer"),
    paused: part<OperationSummary>("paused operations"),
    hosts: part<Host>("hosts", [nas]),
    hostNames: new Map(),
    ...over,
  };
}

describe("summarizeSources", () => {
  it("summarize_reports_failed_sources_by_name_and_keeps_the_rest", () => {
    const retry = vi.fn();
    const state = summarizeSources(
      parts({
        incidents: part<IncidentSummary>("incidents", null, {
          isError: true,
          error: new Error("boom"),
          retry,
        }),
        paused: part("paused operations", [summary("p", "paused")]),
      }),
    );

    expect(state.failed.map((source) => source.name)).toEqual(["incidents"]);
    expect(
      state.items.map((item) => item.key),
      "what loaded is still listed",
    ).toEqual(["operation:p"]);
    state.failed[0]?.retry();
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("summarize_does_not_report_a_failed_refresh_that_still_has_data", () => {
    const state = summarizeSources(
      parts({
        paused: part("paused operations", [summary("p", "paused")], {
          isError: true,
          error: new Error("refresh failed"),
        }),
      }),
    );

    expect(state.failed).toEqual([]);
    expect(state.items).toHaveLength(1);
    expect(state.allLoaded).toBe(true);
  });

  it("summarize_marks_sources_with_more_pages_as_truncated", () => {
    const state = summarizeSources(
      parts({
        incidents: part<IncidentSummary>("incidents", [], { hasMore: true }),
        paused: part<OperationSummary>("paused operations", [], { hasMore: true }),
      }),
    );

    expect(state.truncated).toEqual(["incidents", "paused operations"]);
    expect(summarizeSources(parts()).truncated).toEqual([]);
  });

  it("summarize_is_all_loaded_only_when_every_source_answered", () => {
    expect(summarizeSources(parts()).allLoaded).toBe(true);

    const waiting = summarizeSources(parts({ hosts: part<Host>("hosts", null) }));
    expect(waiting.allLoaded, "a source still loading").toBe(false);
    expect(waiting.pending).toBe(true);

    const failed = summarizeSources(
      parts({ hosts: part<Host>("hosts", null, { isError: true, error: new Error("x") }) }),
    );
    expect(failed.allLoaded, "a failed source is not an answer").toBe(false);
    expect(failed.pending, "a failure is not pending").toBe(false);
  });
});
