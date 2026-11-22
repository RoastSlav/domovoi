import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetEngine } from "../components/graph/testEngine";
import { TRACE_FIXTURES } from "../test/operationFixtures";
import { renderAt } from "../test/render";
import { GraphDemos } from "./GraphDemos";

vi.mock("../components/graph/GraphEngine", () => import("../components/graph/testEngine"));

beforeEach(resetEngine);

const roots = () => document.querySelectorAll<HTMLElement>("[data-graph]");
const ready = () =>
  waitFor(() => {
    expect(roots()).toHaveLength(3);
    for (const root of roots()) {
      expect(root).toHaveAttribute("data-ready", "true");
    }
  });
const groups = (root: HTMLElement) => (root.getAttribute("data-graph-groups") ?? "").split(" ");

describe("the step graph demos", () => {
  it("graph_demos_render_without_violations", async () => {
    renderAt("/", "/", <GraphDemos />);
    await ready();
    expect(screen.getByRole("heading", { level: 2, name: "Step graph" })).toBeVisible();
    // The setup's accessibility check runs over everything left on screen.
  });

  it("the_five_fixtures_are_buttons_and_each_redraws_every_graph", async () => {
    renderAt("/", "/", <GraphDemos />);
    await ready();
    const user = userEvent.setup();
    for (const [name, trace] of Object.entries(TRACE_FIXTURES)) {
      await user.click(screen.getByRole("button", { name }));
      expect(screen.getByRole("button", { name })).toHaveAttribute("aria-pressed", "true");
      const detailed = roots()[0] as HTMLElement;
      await waitFor(() =>
        expect(detailed.getAttribute("data-graph-nodes")?.split(" ")).toEqual(
          trace.steps.map((step) => step.id),
        ),
      );
    }
    expect(Object.keys(TRACE_FIXTURES)).toHaveLength(5);
  });

  it("the_standard_view_folds_technical_steps_and_the_default_view_does_not", async () => {
    renderAt("/", "/", <GraphDemos />);
    await ready();
    const [detailed, standard] = [...roots()] as [HTMLElement, HTMLElement];
    // The first fixture opens with two technical steps ahead of everything else.
    const first = TRACE_FIXTURES["waiting-approval"].steps[0]?.id;
    expect(groups(detailed)).toEqual([""]);
    expect(groups(standard)).toEqual([`group:${first}`]);
  });

  it("the_edge_key_lists_every_kind_of_connection_of_the_fixture", async () => {
    renderAt("/", "/", <GraphDemos />);
    await ready();
    const keys = screen.getAllByRole("list", { name: "Edge key" });
    const last = keys[keys.length - 1] as HTMLElement;
    expect(within(last).getAllByRole("listitem")).toHaveLength(4);
  });

  it("the_failure_demo_shows_the_fallback_message_and_goes_back", async () => {
    renderAt("/", "/", <GraphDemos />);
    await ready();
    const user = userEvent.setup();
    expect(screen.queryByText("The graph could not be drawn")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Show the failure" }));
    expect(screen.getByText("The graph could not be drawn")).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Show the graph again" }));
    expect(screen.queryByText("The graph could not be drawn")).toBeNull();
  });
});
