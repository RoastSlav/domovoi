import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { OperationDetail } from "../../api/types";
import { at } from "../../test/at";
import { resetApi } from "../../test/apiMock";
import { skipAxe } from "../../test/axe";
import {
  operationDetail,
  TRACE_FIXTURES,
  type TraceFixtureName,
} from "../../test/operationFixtures";
import { showOperation } from "../../test/operationPage";
import {
  edgeKindMeta,
  operationStatusMeta,
  stepKindMeta,
  stepOutcomeMeta,
} from "../../design/tone";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const NAMES = Object.keys(TRACE_FIXTURES) as TraceFixtureName[];

function stepRowCount(): number {
  return screen
    .getAllByRole("list", { name: /^(Steps|Cycle \d+)$/ })
    .reduce((sum, list) => sum + within(list).getAllByRole("listitem").length, 0);
}

async function open(detail: OperationDetail) {
  showOperation(detail);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
}

describe.each(NAMES)("operation page for %s", (name) => {
  test(`renders_${name}_without_violations`, async () => {
    const detail = operationDetail(name);
    await open(detail);
    const { trace } = detail;
    expect(
      screen.getAllByText(operationStatusMeta(detail.operation.status).label).length,
    ).toBeGreaterThan(0);
    expect(stepRowCount()).toBe(trace.steps.length);
    for (const kind of new Set(trace.steps.map((step) => step.kind))) {
      expect(screen.getAllByText(stepKindMeta(kind).label).length).toBeGreaterThan(0);
    }
    for (const outcome of new Set(trace.steps.map((step) => step.outcome))) {
      expect(screen.getAllByText(stepOutcomeMeta(outcome).label).length).toBeGreaterThan(0);
    }
    const awaiting = detail.executions.some((execution) => execution.state === "awaiting_approval");
    expect(screen.queryByRole("button", { name: "Approve" }) !== null).toBe(awaiting);
  });
});

describe("what each fixture shows", () => {
  test("waiting_approval_shows_the_banner_and_one_approval_card", async () => {
    await open(operationDetail("waiting-approval"));
    expect(screen.getByText("Waiting for your approval")).toBeInTheDocument();
    const waiting = screen.getByRole("heading", { name: "Waiting for you" }).closest("section");
    expect(waiting).not.toBeNull();
    expect(within(waiting as HTMLElement).getAllByRole("group")).toHaveLength(1);
    expect(within(waiting as HTMLElement).getByRole("button", { name: "Approve" })).toBeVisible();
    expect(screen.getAllByText(stepKindMeta("policy_decision").label).length).toBeGreaterThan(0);
    expect(screen.getAllByText(stepKindMeta("reasoning_turn").label).length).toBeGreaterThan(0);
  });

  test("verified_success_has_no_banner_and_no_decision_buttons", async () => {
    const detail = operationDetail("verified-success");
    await open(detail);
    expect(screen.queryByText(/Waiting for your/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Deny" })).toBeNull();
    const contract = detail.executions[0]?.contract ?? "";
    expect(screen.getAllByText(contract).length).toBeGreaterThan(0);
  });

  test("a_failed_verification_has_two_cycles_and_a_retry_tag", async () => {
    const detail = operationDetail("failed-verification-new-cycle");
    await open(detail);
    expect(screen.getByRole("list", { name: "Cycle 0" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Cycle 1" })).toBeInTheDocument();
    expect(screen.getAllByText("Retry").length).toBeGreaterThan(0);
    expect(screen.getByText(`Cycle ${detail.operation.cycle}`, { selector: "span" })).toBeVisible();
  });

  test("an_unknown_result_pause_offers_show_the_step_resume_and_cancel", async () => {
    await open(operationDetail("outcome-unknown-paused"));
    expect(screen.getByText(/^Result unknown:/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show the step" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Resume operation" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Cancel operation" })).toBeVisible();
    expect(screen.getAllByText("Result unknown").length).toBeGreaterThan(0);
  });

  test("a_budget_pause_names_the_limit_and_offers_to_raise_it", async () => {
    const detail = operationDetail("question-and-budget-pause");
    await open(detail);
    expect(screen.getByText(/limit is spent/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Raise limit and resume" })).toBeVisible();
    for (const outcome of ["refused", "unavailable", "skipped", "cancelled"]) {
      if (detail.trace.steps.some((step) => step.outcome === outcome)) {
        expect(screen.getAllByText(stepOutcomeMeta(outcome).label).length).toBeGreaterThan(0);
      }
    }
  });
});

describe("robustness", () => {
  test("unknown_kind_outcome_and_edge_render_neutral_and_never_throw", async () => {
    const base = operationDetail("verified-success");
    const trace = structuredClone(base.trace);
    trace.steps[0] = { ...at(trace.steps, 0), kind: "from_the_future", outcome: "odd" };
    trace.edges = [
      ...trace.edges,
      { from: at(trace.steps, 1).id, to: at(trace.steps, 0).id, kind: "links" },
    ];
    trace.operation = { ...trace.operation, status: "new_status" };
    const detail = {
      ...base,
      trace,
      operation: { ...base.operation, status: "new_status", pause_reason: { type: "mystery" } },
    } as OperationDetail;
    await open(detail);
    expect(screen.getAllByText("from_the_future").length).toBeGreaterThan(0);
    expect(screen.getAllByText("odd").length).toBeGreaterThan(0);
    expect(edgeKindMeta("links").label).toBe("links");
  });

  test("a_newer_trace_schema_shows_a_warning_and_still_renders", async () => {
    const base = operationDetail("verified-success");
    await open({ ...base, trace: { ...base.trace, schema_version: 2 } } as OperationDetail);
    expect(screen.getByText(/built for an older operation format/)).toBeInTheDocument();
    expect(stepRowCount()).toBe(base.trace.steps.length);
  });

  test("a_trace_with_300_steps_shows_the_newest_200_and_every_open_one", async () => {
    // Axe over 200 rows is too slow for a test hook; the rows are the same as in the other tests.
    skipAxe();
    const base = operationDetail("verified-success");
    const steps = Array.from({ length: 300 }, (_, index) => ({
      ...at(base.trace.steps, 0),
      id: `step-${index}`,
      outcome: index === 3 ? "running" : "succeeded",
    }));
    await open({ ...base, trace: { ...base.trace, steps, edges: [] }, executions: [] });
    expect(stepRowCount()).toBe(201);
    // The Graph view has its own button above the graph, and the list below has another.
    const earlier = screen.getAllByRole("button", { name: "Show 99 earlier steps" });
    expect(earlier.length).toBeGreaterThan(0);
    expect(earlier[0]).toBeVisible();
  });
});
