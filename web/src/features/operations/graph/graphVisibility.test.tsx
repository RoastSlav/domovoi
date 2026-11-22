import { act, fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TraceStep } from "../../../api/operationTrace";
import { resetApi } from "../../../test/apiMock";
import { resetEngine } from "../../../components/graph/testEngine";
import { drawnGroups, drawnNodes, openGraphPage } from "../../../test/graphPage";
import {
  operationDetail,
  TRACE_FIXTURES,
  type TraceFixtureName,
} from "../../../test/operationFixtures";
import { PROFILE_IDS, type ProfileId } from "../../../design/visibility";

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

// Written from the product rules, not from the module under test: these kinds are always P0,
// and any outcome outside the quiet ones makes a step P0.
const ALWAYS_P0_KINDS = new Set(["approval", "execution", "verification", "question"]);
const QUIET_OUTCOMES = new Set(["succeeded", "running", "pending", "skipped", "cancelled"]);
const TECHNICAL_KINDS = new Set(["evidence_request", "reasoning_turn"]);
const isP0 = (step: TraceStep) =>
  ALWAYS_P0_KINDS.has(step.kind) || !QUIET_OUTCOMES.has(step.outcome);
const isTechnical = (step: TraceStep) =>
  TECHNICAL_KINDS.has(step.kind) && QUIET_OUTCOMES.has(step.outcome);

const rowCount = (category?: string) =>
  [...document.querySelectorAll<HTMLElement>('[data-visibility-id="operation.step"]')].filter(
    (row) => category === undefined || row.dataset.visibility === category,
  ).length;

describe("the graph at each detail level", () => {
  it.each(FIXTURES)(
    "p0_steps_are_drawn_in_every_profile (%s)",
    async (name) => {
      const detail = operationDetail(name);
      const p0 = detail.trace.steps.filter(isP0).map((step) => step.id);
      expect(p0.length).toBeGreaterThan(0);
      for (const profile of PROFILE_IDS) {
        const { root, unmount } = await openGraphPage(detail, { profile });
        const drawn = drawnNodes(root);
        for (const id of p0) {
          expect(drawn, `${id} is missing in ${profile}`).toContain(id);
        }
        unmount();
      }
    },
    SLOW,
  );

  it.each(FIXTURES)(
    "detailed_profile_has_no_groups (%s)",
    async (name) => {
      const detail = operationDetail(name);
      const { root } = await openGraphPage(detail, { profile: "detailed" });
      expect(drawnGroups(root)).toEqual([]);
      expect(drawnNodes(root)).toEqual(detail.trace.steps.map((step) => step.id));
    },
    SLOW,
  );

  it.each(["standard", "essential"] as ProfileId[])(
    "%s_profile_folds_each_technical_run_into_one_group_and_draws_the_rest",
    async (profile) => {
      const detail = operationDetail("waiting-approval");
      const { root } = await openGraphPage(detail, { profile });
      const [first, second] = detail.trace.steps;
      expect(first && second && isTechnical(first) && isTechnical(second)).toBe(true);
      expect(drawnGroups(root)).toEqual([`group:${first?.id}`]);
      const rest = detail.trace.steps.slice(2).map((step) => step.id);
      expect(drawnNodes(root)).toEqual([`group:${first?.id}`, ...rest]);
    },
    SLOW,
  );

  it(
    "standard_profile_shows_a_folded_group_and_open_group_shows_members",
    async () => {
      const detail = operationDetail("waiting-approval");
      const [first, second] = detail.trace.steps.map((step) => step.id);
      const group = `group:${first}`;
      const { root } = await openGraphPage(detail, { profile: "standard" });
      expect(drawnGroups(root)).toEqual([group]);
      expect(screen.queryByRole("button", { name: "Fold technical steps" })).toBeNull();

      // Home goes to the first node, which is the group; Enter opens it.
      act(() => root.focus());
      fireEvent.keyDown(root, { key: "Home" });
      expect(root).toHaveAttribute("data-focused-node", group);
      fireEvent.keyDown(root, { key: "Enter" });
      expect(drawnGroups(root)).toEqual([]);
      expect(drawnNodes(root).slice(0, 2)).toEqual([first, second]);
      expect(drawnNodes(root)).not.toContain(group);

      await userEvent.setup().click(screen.getByRole("button", { name: "Fold technical steps" }));
      expect(drawnGroups(root)).toEqual([group]);
      expect(drawnNodes(root)).not.toContain(first);
      expect(screen.queryByRole("button", { name: "Fold technical steps" })).toBeNull();
    },
    SLOW,
  );

  it.each(FIXTURES)(
    "a_step_list_below_the_graph_always_lists_every_visible_step (%s)",
    async (name) => {
      const detail = operationDetail(name);
      const steps = detail.trace.steps;
      for (const profile of PROFILE_IDS) {
        const { unmount } = await openGraphPage(detail, { profile });
        // Whatever the graph folds, the list keeps a row for every P0 step; Detailed lists all.
        expect(rowCount("P0"), profile).toBe(steps.filter(isP0).length);
        if (profile === "detailed") {
          expect(rowCount()).toBe(steps.length);
        }
        unmount();
      }
    },
    SLOW,
  );
});
