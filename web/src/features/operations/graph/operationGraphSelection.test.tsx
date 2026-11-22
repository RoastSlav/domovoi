import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { engineSpy, resetEngine } from "../../../components/graph/testEngine";
import { resetApi } from "../../../test/apiMock";
import {
  address,
  drawnGroups,
  drawnNodes,
  openGraphPage,
  outlineLink,
} from "../../../test/graphPage";
import { operationDetail } from "../../../test/operationFixtures";

vi.mock("../../../api/client", () => import("../../../test/apiMock"));
vi.mock(
  "../../../components/graph/GraphEngine",
  () => import("../../../components/graph/testEngine"),
);

beforeEach(() => {
  resetApi();
  resetEngine();
});

const SLOW = 40_000;
const detail = operationDetail("waiting-approval");
const ids = detail.trace.steps.map((step) => step.id);
const PLAN = ids[3] as string;
const technical = ids[1] as string;
const query = (id: string) => `?step=${id}`;
const inspector = () => screen.findByRole("complementary", { name: "Step inspector" });
const noInspector = () => screen.queryByRole("complementary", { name: "Step inspector" });
const press = (root: HTMLElement, key: string) => fireEvent.keyDown(root, { key });
const focusRoot = (root: HTMLElement) => act(() => root.focus());
const pointerSelect = (id: string) => act(() => engineSpy.props?.onPointerSelect(id));

describe("selecting steps in the graph", () => {
  it(
    "clicking_a_graph_node_selects_the_step_in_the_url_and_the_outline",
    async () => {
      const { root } = await openGraphPage(detail);
      pointerSelect(PLAN);
      await inspector();
      expect(address()).toContain(query(PLAN));
      expect(outlineLink(PLAN)).toHaveAttribute("aria-current", "true");
      expect(outlineLink(ids[0] as string)).not.toHaveAttribute("aria-current");
      expect(root).toHaveAttribute("data-selected-step", PLAN);
    },
    SLOW,
  );

  it(
    "clicking_an_outline_row_selects_the_node_in_the_graph",
    async () => {
      const { root } = await openGraphPage(detail);
      await userEvent.setup().click(outlineLink(PLAN));
      await waitFor(() => expect(root).toHaveAttribute("data-selected-step", PLAN));
      expect(engineSpy.props?.selectedId).toBe(PLAN);
      expect(address()).toContain(query(PLAN));
    },
    SLOW,
  );

  it(
    "arrow_keys_do_not_change_the_url_or_open_the_inspector",
    async () => {
      const { root } = await openGraphPage(detail);
      const before = address();
      focusRoot(root);
      const entry = root.getAttribute("data-focused-node");
      expect(entry).toBeTruthy();
      for (let i = 0; i < 3; i += 1) {
        press(root, "ArrowRight");
      }
      press(root, "ArrowDown");
      expect(root).toHaveAttribute("data-focused-node");
      expect(address()).toBe(before);
      expect(noInspector()).toBeNull();
      expect(root).toHaveAttribute("data-selected-step", "");
    },
    SLOW,
  );

  it(
    "enter_selects_opens_and_moves_focus_into_the_inspector",
    async () => {
      const { root } = await openGraphPage(detail);
      focusRoot(root);
      const target = root.getAttribute("data-focused-node") as string;
      press(root, "Enter");
      const panel = await inspector();
      expect(address()).toContain(query(target));
      await waitFor(() => expect(panel).toContainElement(document.activeElement as HTMLElement));
      await userEvent.setup().keyboard("{Escape}");
      await waitFor(() => expect(root).toHaveFocus());
      expect(address()).not.toContain("step=");
      expect(noInspector()).toBeNull();
    },
    SLOW,
  );

  it(
    "enter_on_the_already_selected_step_moves_focus_into_the_open_inspector",
    async () => {
      const { root } = await openGraphPage(detail, { search: query(PLAN) });
      const panel = await inspector();
      focusRoot(root);
      expect(root).toHaveFocus();
      expect(root).toHaveAttribute("data-focused-node", PLAN);
      press(root, "Enter");
      await waitFor(() => expect(panel).toContainElement(document.activeElement as HTMLElement));
      expect(address()).toContain(query(PLAN));
    },
    SLOW,
  );

  it(
    "escape_on_the_graph_with_a_selection_clears_it_and_closes_the_inspector",
    async () => {
      const { root } = await openGraphPage(detail);
      pointerSelect(PLAN);
      await inspector();
      focusRoot(root);
      press(root, "Escape");
      await waitFor(() => expect(noInspector()).toBeNull());
      expect(address()).not.toContain("step=");
      expect(root).toHaveAttribute("data-selected-step", "");
      await waitFor(() => expect(root).toHaveFocus());
    },
    SLOW,
  );
});

describe("a step in the address", () => {
  it(
    "a_deep_link_to_a_step_in_a_folded_group_opens_the_group_and_selects_it",
    async () => {
      const { root } = await openGraphPage(detail, {
        profile: "standard",
        search: query(technical),
      });
      expect(drawnGroups(root)).toEqual([]);
      expect(drawnNodes(root)).toContain(technical);
      expect(root).toHaveAttribute("data-selected-step", technical);
      await inspector();
    },
    SLOW,
  );

  it(
    "a_deep_link_does_not_take_focus",
    async () => {
      await openGraphPage(detail, { search: query(PLAN) });
      const panel = await inspector();
      expect(panel).not.toContainElement(document.activeElement as HTMLElement);
      expect(document.body).toHaveFocus();
    },
    SLOW,
  );

  it(
    "an_invalid_step_param_is_dropped",
    async () => {
      const { root } = await openGraphPage(detail, { search: "?step=nope" });
      await waitFor(() => expect(address()).not.toContain("step="));
      expect(root).toHaveAttribute("data-selected-step", "");
      expect(noInspector()).toBeNull();
    },
    SLOW,
  );
});
