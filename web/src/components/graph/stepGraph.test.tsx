import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OperationTrace } from "../../api/operationTrace";
import { buildGraphModel } from "../../features/operations/graph/graphModel";
import { TRACE_FIXTURES } from "../../test/operationFixtures";
import { expectNoA11yViolations } from "../../test/axe";
import { makeTrace } from "../../test/syntheticTrace";
import { StepGraph } from "./StepGraph";
import { engineSpy, resetEngine } from "./testEngine";
import type { GraphModel, StepGraphProps } from "./types";
import type { ProfileId } from "../../design/visibility";

vi.mock("./GraphEngine", () => import("./testEngine"));

const WAITING = TRACE_FIXTURES["waiting-approval"] as OperationTrace;

function modelOf(
  trace: OperationTrace,
  profile: ProfileId = "detailed",
  expanded: string[] = [],
): GraphModel {
  return buildGraphModel({
    trace,
    profile,
    expanded: new Set(expanded),
    selected: null,
    showAll: false,
  }).model;
}

function withOutcome(trace: OperationTrace, id: string, outcome: string): OperationTrace {
  return { ...trace, steps: trace.steps.map((s) => (s.id === id ? { ...s, outcome } : s)) };
}

const noop = () => undefined;
function props(model: GraphModel, over: Partial<StepGraphProps> = {}): StepGraphProps {
  return {
    model,
    selectedId: null,
    onSelect: vi.fn(),
    onOpen: vi.fn(),
    onClear: vi.fn(),
    onToggleGroup: vi.fn(),
    ...over,
  };
}

async function mount(p: StepGraphProps) {
  const view = render(<StepGraph {...p} />);
  const root = await screen.findByRole("group", { name: "Step graph" });
  await vi.waitFor(() => expect(root).toHaveAttribute("data-ready", "true"));
  return { ...view, root };
}

const focus = (root: HTMLElement) => act(() => root.focus());
const press = (root: HTMLElement, key: string, extra: object = {}) =>
  fireEvent.keyDown(root, { key, ...extra });
const status = () => screen.getByRole("status");

beforeEach(resetEngine);

describe("the step graph shell", () => {
  it("graph_root_is_one_tab_stop", async () => {
    const user = userEvent.setup();
    const p = props(modelOf(WAITING), { onFoldAll: noop });
    const { root } = await mount(p);
    const after = document.createElement("button");
    after.textContent = "After";
    document.body.append(after);
    await user.tab();
    const order: string[] = [];
    for (let i = 0; i < 5; i += 1) {
      order.push(
        document.activeElement?.getAttribute("aria-label") ??
          document.activeElement?.textContent ??
          "",
      );
      await user.tab();
    }
    expect(order.slice(0, 3)).toEqual(["Fit to view", "Zoom in", "Zoom out"]);
    expect(order[3]).toBe("Fold technical steps");
    expect(order[4]).toBe("Step graph");
    expect(document.activeElement).toBe(after);
    const tabbable = "button, a[href], input, select, textarea, [tabindex]:not([tabindex='-1'])";
    expect(
      [root, ...root.querySelectorAll(tabbable)].filter((el) => el.matches(tabbable)),
    ).toHaveLength(1);
    after.remove();
  });

  it("graph_root_has_a_name_a_description_and_a_status_line", async () => {
    const { root } = await mount(props(modelOf(WAITING)));
    expect(root).toHaveAttribute("aria-roledescription", "graph");
    const help = document.getElementById(root.getAttribute("aria-describedby") ?? "");
    expect(help).toHaveTextContent("Arrow keys move between connected steps.");
    expect(help).toHaveTextContent("Plus, minus and zero zoom and fit.");
    expect(status()).toBeInTheDocument();
    expect(root).toHaveAttribute("data-variant", "svg-dagre");
  });

  it("arrow_keys_move_focus_and_announce_without_selecting", async () => {
    const p = props(modelOf(WAITING));
    const { root } = await mount(p);
    focus(root);
    press(root, "End");
    const first = root.getAttribute("data-focused-node");
    press(root, "ArrowLeft");
    const next = root.getAttribute("data-focused-node");
    expect(next).not.toBe(first);
    const node = p.model.nodes.find((n) => n.id === next);
    expect(status()).toHaveTextContent(node?.title ?? "never");
    expect(p.onSelect).not.toHaveBeenCalled();
    expect(p.onOpen).not.toHaveBeenCalled();
  });

  it("enter_opens_the_focused_step", async () => {
    const p = props(modelOf(WAITING));
    const { root } = await mount(p);
    focus(root);
    press(root, "Home");
    press(root, "Enter");
    expect(p.onOpen).toHaveBeenCalledTimes(1);
    expect(p.onOpen).toHaveBeenCalledWith(p.model.nodes[0]?.id);
    expect(p.onToggleGroup).not.toHaveBeenCalled();
  });

  it("enter_on_a_group_node_toggles_the_group", async () => {
    const p = props(modelOf(makeTrace(30, 1), "standard"));
    const { root } = await mount(p);
    const group = p.model.nodes.find((n) => n.kind === "group");
    expect(group).toBeDefined();
    focus(root);
    press(root, "Home");
    expect(root.getAttribute("data-focused-node")).toBe(group?.id);
    press(root, "Enter");
    expect(p.onToggleGroup).toHaveBeenCalledWith(group?.id);
    expect(p.onOpen).not.toHaveBeenCalled();
    expect(status()).toHaveTextContent("Press Enter to show them.");
  });

  it("a_click_on_a_group_node_toggles_it_instead_of_selecting", async () => {
    const p = props(modelOf(makeTrace(30, 1), "standard"));
    await mount(p);
    const group = p.model.nodes.find((n) => n.kind === "group");
    act(() => engineSpy.props?.onPointerSelect(group?.id ?? ""));
    expect(p.onToggleGroup).toHaveBeenCalledWith(group?.id);
    expect(p.onSelect).not.toHaveBeenCalled();
  });

  it("step_graph_has_no_axe_violations", async () => {
    // setup.ts runs axe over what each test leaves on screen: the drawn graph, then the fallback.
    const first = await mount(props(modelOf(WAITING), { onFoldAll: noop }));
    focus(first.root);
    press(first.root, "Home");
    await expectNoA11yViolations(document.body);
    first.unmount();
    engineSpy.failure = "boom";
    render(<StepGraph {...props(modelOf(WAITING))} />);
    expect(await screen.findByText("The graph could not be drawn")).toBeInTheDocument();
    await expectNoA11yViolations(document.body);
  });

  it("focus_moves_to_the_first_member_when_a_focused_group_opens", async () => {
    const trace = makeTrace(30, 1);
    const folded = props(modelOf(trace, "standard"));
    const { root, rerender } = await mount(folded);
    const group = folded.model.nodes.find((n) => n.kind === "group");
    focus(root);
    act(() => engineSpy.props?.onPointerSelect(group?.id ?? ""));
    expect(root.getAttribute("data-focused-node")).toBe(group?.id);
    press(root, "Enter");
    expect(folded.onToggleGroup).toHaveBeenCalled();
    const open = modelOf(trace, "standard", [group?.id ?? ""]);
    rerender(<StepGraph {...folded} model={open} />);
    const first = group?.members[0] ?? "";
    await vi.waitFor(() => expect(root.getAttribute("data-focused-node")).toBe(first));
    const title = open.nodes.find((n) => n.id === first)?.title ?? "never";
    expect(status()).toHaveTextContent(title);
  });

  it("double_click_on_a_node_opens_it_and_click_selects_it", async () => {
    const p = props(modelOf(WAITING));
    await mount(p);
    const id = p.model.nodes[1]?.id ?? "";
    act(() => engineSpy.props?.onPointerSelect(id));
    expect(p.onSelect).toHaveBeenCalledWith(id);
    act(() => engineSpy.props?.onPointerOpen(id));
    expect(p.onOpen).toHaveBeenCalledWith(id);
    act(() => engineSpy.props?.onBackground());
    expect(p.onClear).toHaveBeenCalledTimes(1);
  });

  it("escape_clears_the_selection_and_is_not_consumed_without_one", async () => {
    const id = modelOf(WAITING).nodes[0]?.id ?? "";
    const withSelection = props(modelOf(WAITING), { selectedId: id });
    const first = await mount(withSelection);
    expect(press(first.root, "Escape")).toBe(false);
    expect(withSelection.onClear).toHaveBeenCalledTimes(1);
    first.unmount();
    const without = props(modelOf(WAITING));
    const second = await mount(without);
    expect(press(second.root, "Escape")).toBe(true);
    expect(without.onClear).not.toHaveBeenCalled();
  });

  it("plus_minus_and_zero_zoom_and_fit", async () => {
    const { root } = await mount(props(modelOf(WAITING)));
    press(root, "+");
    press(root, "-");
    press(root, "0");
    expect(engineSpy.handle.zoomBy).toHaveBeenNthCalledWith(1, 1.25);
    expect(engineSpy.handle.zoomBy).toHaveBeenNthCalledWith(2, 0.8);
    expect(engineSpy.handle.fit).toHaveBeenCalledTimes(1);
  });

  it("keys_with_modifiers_pass_through", async () => {
    const { root } = await mount(props(modelOf(WAITING)));
    focus(root);
    press(root, "Home");
    const at = root.getAttribute("data-focused-node");
    expect(press(root, "ArrowRight", { ctrlKey: true })).toBe(true);
    expect(press(root, "End", { altKey: true })).toBe(true);
    expect(press(root, "ArrowLeft", { metaKey: true })).toBe(true);
    expect(root.getAttribute("data-focused-node")).toBe(at);
  });

  it("page_never_scrolls_under_a_handled_key", async () => {
    const { root } = await mount(props(modelOf(WAITING)));
    focus(root);
    const keys = ["Home", "ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown", "PageDown", "PageUp"];
    for (const key of [...keys, "End", "a", "+", "=", "-", "0", "Enter"]) {
      expect(press(root, key), key).toBe(false);
    }
  });

  it("focus_follows_a_selection_made_elsewhere", async () => {
    const p = props(modelOf(WAITING));
    const { root, rerender } = await mount(p);
    const id = p.model.nodes[2]?.id ?? "";
    rerender(<StepGraph {...p} selectedId={id} />);
    await vi.waitFor(() => expect(root.getAttribute("data-focused-node")).toBe(id));
    expect(engineSpy.handle.reveal).toHaveBeenCalledWith(id);
  });

  it("selected_step_is_exposed_as_data_selected_step", async () => {
    const p = props(modelOf(WAITING));
    const { root, rerender } = await mount(p);
    expect(root).toHaveAttribute("data-selected-step", "");
    const id = p.model.nodes[1]?.id ?? "";
    rerender(<StepGraph {...p} selectedId={id} />);
    expect(root).toHaveAttribute("data-selected-step", id);
  });

  it("data_graph_nodes_lists_the_model_nodes_in_order", async () => {
    const p = props(modelOf(WAITING));
    const { root } = await mount(p);
    expect(root.getAttribute("data-graph-nodes")).toBe(p.model.nodes.map((n) => n.id).join(" "));
    expect(root.getAttribute("data-layout-key")).toBe(p.model.layoutKey);
  });

  it("toolbar_buttons_are_labelled_and_fold_appears_only_with_onFoldAll", async () => {
    const first = await mount(props(modelOf(WAITING)));
    const bar = screen.getByRole("toolbar", { name: "Graph controls" });
    expect(
      within(bar)
        .getAllByRole("button")
        .map((b) => b.getAttribute("aria-label") ?? b.textContent),
    ).toEqual(["Fit to view", "Zoom in", "Zoom out"]);
    first.unmount();
    const onFoldAll = vi.fn();
    await mount(props(modelOf(WAITING), { onFoldAll }));
    await userEvent.click(screen.getByRole("button", { name: "Fold technical steps" }));
    expect(onFoldAll).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(engineSpy.handle.zoomBy).toHaveBeenCalledWith(1.25);
  });

  it("edge_key_lists_only_the_kinds_present_and_uses_no_inline_style", async () => {
    const base = modelOf(WAITING);
    const kinds = ["depends_on", "caused_by", "verifies", "retry_of", "answers"];
    const edgesOf = (list: string[]) =>
      list.map((kind, i) => ({ id: `e${i}`, from: "b", to: "a", kind, label: kind }));
    const model = (list: string[]): GraphModel => ({
      ...base,
      nodes: base.nodes.slice(0, 2).map((n, i) => ({ ...n, id: i === 0 ? "a" : "b" })),
      edges: edgesOf(list),
      layoutKey: list.join(),
    });
    const first = await mount(props(model(kinds)));
    const key = screen.getByRole("list", { name: "Edge key" });
    expect(within(key).getAllByRole("listitem")).toHaveLength(5);
    expect(key.querySelectorAll("[style]")).toHaveLength(0);
    expect(key.querySelectorAll("line[stroke-dasharray]").length).toBeGreaterThan(0);
    first.unmount();
    await mount(props(model(["depends_on", "answers"])));
    expect(
      within(screen.getByRole("list", { name: "Edge key" })).getAllByRole("listitem"),
    ).toHaveLength(2);
  });

  it("engine_failure_shows_the_fallback_and_try_again_retries", async () => {
    engineSpy.failure = "boom";
    render(<StepGraph {...props(modelOf(WAITING))} />);
    expect(await screen.findByText("The graph could not be drawn")).toBeInTheDocument();
    expect(screen.getByText(/The step list below has the same steps\./)).toBeInTheDocument();
    expect(screen.queryByText(/reload the page/)).toBeNull();
    const mounts = engineSpy.mounts;
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByText(/If it keeps failing, reload the page\./)).toBeInTheDocument();
    expect(engineSpy.mounts).toBeGreaterThan(mounts);
  });
});

describe("transitions and layout animation", () => {
  const lastId = WAITING.steps[WAITING.steps.length - 1]?.id ?? "";
  const changed = withOutcome(WAITING, lastId, "failed");

  it("transitions_prop_follows_the_plan_and_resets_with_the_remember_key", async () => {
    const a = props(modelOf(WAITING));
    const { rerender } = await mount(a);
    expect(engineSpy.props?.transitions.size).toBe(0);
    rerender(<StepGraph {...a} model={modelOf(changed)} />);
    expect([...(engineSpy.props?.transitions ?? [])]).toEqual([lastId]);
    rerender(<StepGraph {...a} model={modelOf(changed)} />);
    expect(engineSpy.props?.transitions.size).toBe(0);
    rerender(<StepGraph {...a} model={modelOf(WAITING)} />);
    expect(engineSpy.props?.transitions.size).toBe(0);
    rerender(<StepGraph {...a} rememberKey="other" model={modelOf(changed)} />);
    expect([...(engineSpy.props?.transitions ?? [])]).toEqual([lastId]);
  });

  it("animate_false_gives_no_transitions_and_no_layout_animation", async () => {
    const a = props(modelOf(WAITING), { animate: false });
    const { rerender } = await mount(a);
    rerender(<StepGraph {...a} model={modelOf(changed)} />);
    expect(engineSpy.props?.transitions.size).toBe(0);
    expect(engineSpy.props?.animateLayout).toBe(false);
  });

  it("layout_animation_is_off_above_150_nodes", async () => {
    const big = await mount(props(modelOf(makeTrace(200, 1))));
    expect(engineSpy.props?.animateLayout).toBe(false);
    big.unmount();
    await mount(props(modelOf(makeTrace(50, 1))));
    expect(engineSpy.props?.animateLayout).toBe(true);
  });
});

describe("untrusted text", () => {
  it("node_text_is_text_not_markup", async () => {
    const hostile = [
      '<img src=x onerror="window.__x=1">',
      "<script>window.__x=1</script>",
      '"><svg onload=window.__x=1>',
    ];
    const base = modelOf(WAITING);
    const model: GraphModel = {
      ...base,
      nodes: base.nodes.slice(0, 3).map((n, i) => ({ ...n, summary: hostile[i] ?? null })),
      edges: [],
      layoutKey: "hostile",
    };
    const { root, container } = await mount(props(model));
    focus(root);
    press(root, "Home");
    expect(status()).toHaveTextContent(hostile[0] ?? "never");
    for (const text of hostile) {
      expect(container).toHaveTextContent(text);
    }
    expect(document.querySelector("img[src='x']")).toBeNull();
    expect(document.querySelector("script")).toBeNull();
    expect(Reflect.get(window, "__x")).toBeUndefined();
  });
});
