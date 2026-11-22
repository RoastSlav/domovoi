import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OperationDetail } from "../../../api/types";
import { resetEngine } from "../../../components/graph/testEngine";
import { resetApi } from "../../../test/apiMock";
import { skipAxe } from "../../../test/axe";
import { stubMedia } from "../../../test/fixtures";
import { drawnNodes, graphRoot, openGraphPage } from "../../../test/graphPage";
import {
  operationDetail,
  TRACE_FIXTURES,
  type TraceFixtureName,
} from "../../../test/operationFixtures";
import { showOperation } from "../../../test/operationPage";
import { makeTrace } from "../../../test/syntheticTrace";

vi.mock("../../../api/client", () => import("../../../test/apiMock"));
vi.mock(
  "../../../components/graph/GraphEngine",
  () => import("../../../components/graph/testEngine"),
);

beforeEach(() => {
  resetApi();
  resetEngine();
});

const FIXTURES = Object.keys(TRACE_FIXTURES) as TraceFixtureName[];
const SLOW = 40_000;
const stepLists = () => screen.getAllByRole("list", { name: /^(Steps|Cycle \d+)$/ });
const stepRows = () =>
  stepLists().reduce((n, list) => n + within(list).getAllByRole("listitem").length, 0);

function longDetail(): OperationDetail {
  const base = operationDetail("verified-success");
  return { ...base, trace: makeTrace(300, 1), executions: [] } as OperationDetail;
}

describe.each(FIXTURES)("the Graph view of %s", (name) => {
  it(
    `renders_${name}_in_graph_view_without_violations`,
    async () => {
      const detail = operationDetail(name);
      const { root } = await openGraphPage(detail);
      expect(drawnNodes(root)).toEqual(detail.trace.steps.map((step) => step.id));
      expect(screen.getByRole("radio", { name: "Graph" })).toBeChecked();
      expect(stepRows()).toBe(detail.trace.steps.length);
      // The axe check in the test setup runs over everything left on screen.
    },
    SLOW,
  );
});

describe("the view switch", () => {
  it("narrow_screens_open_in_timeline_and_the_graph_option_is_still_offered", async () => {
    const detail = operationDetail("verified-success");
    showOperation(detail);
    // The page reads the media query on every render, so the data's arrival picks this up.
    stubMedia(false);
    await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
    expect(graphRoot()).toBeNull();
    expect(screen.getByRole("radio", { name: "Timeline" })).toBeChecked();
    await userEvent.setup().click(screen.getByRole("radio", { name: "Graph" }));
    expect(await screen.findByRole("group", { name: "Step graph" })).toBeInTheDocument();
    expect(graphRoot()).not.toBeNull();
  });

  it("view_switch_offers_timeline_tree_and_graph", async () => {
    await openGraphPage(operationDetail("verified-success"));
    const control = screen.getByRole("group", { name: "View" });
    const options = within(control).getAllByRole("radio");
    expect(options.map((option) => option.closest("label")?.textContent)).toEqual([
      "Timeline",
      "Tree",
      "Graph",
    ]);
  });

  it("choosing_timeline_removes_the_graph_and_choosing_graph_brings_it_back", async () => {
    const { root } = await openGraphPage(operationDetail("verified-success"));
    const user = userEvent.setup();
    await user.click(screen.getByRole("radio", { name: "Timeline" }));
    expect(root.isConnected).toBe(false);
    expect(graphRoot()).toBeNull();
    await user.click(screen.getByRole("radio", { name: "Graph" }));
    expect(await screen.findByRole("group", { name: "Step graph" })).toBeInTheDocument();
  });
});

describe("what the graph is made of", () => {
  it(
    "show_earlier_steps_button_appears_for_a_long_trace_and_reveals_the_rest",
    async () => {
      // 300 outline rows are too many for the axe check's time budget in a test hook.
      skipAxe();
      const detail = longDetail();
      const { root } = await openGraphPage(detail);
      // The newest 200 steps plus the four pending ones among the older hundred.
      expect(drawnNodes(root)).toHaveLength(204);
      const [graphButton, listButton] = screen.getAllByRole("button", {
        name: "Show 96 earlier steps",
      });
      expect(listButton).toBeDefined();
      // The graph's own button comes first, above the graph.
      expect(graphButton?.compareDocumentPosition(root)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
      await act(async () => graphButton?.click());
      expect(drawnNodes(root)).toHaveLength(300);
      expect(screen.getAllByRole("button", { name: /earlier steps/ })).toHaveLength(1);
    },
    SLOW,
  );

  it("empty_trace_shows_no_steps_yet", async () => {
    const base = operationDetail("verified-success");
    const detail = {
      ...base,
      trace: { ...base.trace, steps: [], edges: [] },
      executions: [],
    } as OperationDetail;
    showOperation(detail);
    await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
    expect(screen.getByText("No steps yet")).toBeVisible();
    expect(graphRoot()).toBeNull();
  });

  it("graph_code_has_no_decision_controls", async () => {
    const { root } = await openGraphPage(operationDetail("waiting-approval"));
    const frame = root.closest<HTMLElement>("[data-graph-frame]") as HTMLElement;
    // The page does offer a decision, just not inside the graph.
    expect(screen.getAllByRole("button", { name: "Approve" }).length).toBeGreaterThan(0);
    expect(within(frame).getAllByRole("button").length).toBeGreaterThan(0);
    expect(
      within(frame).queryByRole("button", { name: /^(Approve|Deny|Resume|Cancel|Answer)/ }),
    ).toBeNull();
  });
});
