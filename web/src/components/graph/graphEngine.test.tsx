import { act, render } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";
import type { OperationTrace } from "../../api/operationTrace";
import { buildGraphModel } from "../../features/operations/graph/graphModel";
import { TRACE_FIXTURES } from "../../test/operationFixtures";
import GraphEngine from "./GraphEngine";
import { NODE_H, NODE_W } from "./layout";
import type { GraphEngineHandle, GraphEngineProps, GraphModel } from "./types";

const WAITING = TRACE_FIXTURES["waiting-approval"] as OperationTrace;

function modelOf(trace: OperationTrace): GraphModel {
  return buildGraphModel({
    trace,
    profile: "detailed",
    expanded: new Set(),
    selected: null,
    showAll: false,
  }).model;
}

function engineProps(model: GraphModel, over: Partial<GraphEngineProps> = {}): GraphEngineProps {
  return {
    model,
    focusedId: null,
    selectedId: null,
    rootFocused: false,
    transitions: new Set(),
    animateLayout: false,
    onPointerSelect: vi.fn(),
    onPointerOpen: vi.fn(),
    onBackground: vi.fn(),
    onLayout: vi.fn(),
    onFailure: vi.fn(),
    ...over,
  };
}

describe("the svg engine in jsdom", () => {
  it("draws every node and edge as svg elements and writes no style attribute", () => {
    const model = modelOf(WAITING);
    const props = engineProps(model, { selectedId: model.nodes[0]?.id ?? null });
    const { container } = render(<GraphEngine {...props} />);
    expect(container.querySelectorAll("svg > g > g[data-node-id]")).toHaveLength(
      model.nodes.length,
    );
    expect(container.querySelectorAll("path[data-edge-id]")).toHaveLength(model.edges.length);
    expect(container.querySelectorAll("[style]")).toHaveLength(0);
    expect(container.querySelectorAll("style")).toHaveLength(0);
    expect(container.querySelector("foreignObject")).toBeNull();
    expect(props.onFailure).not.toHaveBeenCalled();
  });

  it("reports one position per node after the layout, none overlapping, left to right", () => {
    const model = modelOf(WAITING);
    const props = engineProps(model);
    render(<GraphEngine {...props} />);
    expect(props.onLayout).toHaveBeenCalledTimes(1);
    const [positions, key] = vi.mocked(props.onLayout).mock.calls[0] ?? [[], ""];
    expect(key).toBe(model.layoutKey);
    expect(positions).toHaveLength(model.nodes.length);
    for (const a of positions) {
      for (const b of positions) {
        if (a.id !== b.id) {
          expect(Math.abs(a.x - b.x) >= NODE_W || Math.abs(a.y - b.y) >= NODE_H).toBe(true);
        }
      }
    }
    for (const edge of model.edges) {
      const from = positions.find((p) => p.id === edge.from);
      const to = positions.find((p) => p.id === edge.to);
      expect(to && from && to.x < from.x).toBe(true);
    }
  });

  it("does not lay out again for a status-only change but does for a new step", () => {
    const model = modelOf(WAITING);
    const props = engineProps(model);
    const { rerender } = render(<GraphEngine {...props} />);
    const last = WAITING.steps[WAITING.steps.length - 1];
    const changed = modelOf({
      ...WAITING,
      steps: WAITING.steps.map((s) => (s.id === last?.id ? { ...s, outcome: "failed" } : s)),
    });
    rerender(<GraphEngine {...props} model={changed} />);
    expect(props.onLayout).toHaveBeenCalledTimes(1);
    const bigger = modelOf(WAITING);
    rerender(
      <GraphEngine
        {...props}
        model={{ ...bigger, nodes: bigger.nodes.slice(1), layoutKey: "fewer" }}
      />,
    );
    expect(props.onLayout).toHaveBeenCalledTimes(2);
  });

  it("draws summaries as text and answers the handle", () => {
    const hostile = '<img src=x onerror="window.__x=1">';
    const base = modelOf(WAITING);
    const model: GraphModel = {
      ...base,
      nodes: base.nodes.map((n, i) => (i === 0 ? { ...n, summary: hostile } : n)),
    };
    const ref = createRef<GraphEngineHandle>();
    const { container } = render(<GraphEngine {...engineProps(model)} ref={ref} />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("<img src=x");
    expect(Reflect.get(window, "__x")).toBeUndefined();
    expect(ref.current?.positions()).toHaveLength(model.nodes.length);
    act(() => {
      ref.current?.zoomBy(1.25);
      ref.current?.reveal(model.nodes[0]?.id ?? "");
      ref.current?.fit();
    });
    expect(container.querySelector("svg")?.getAttribute("viewBox")).toMatch(/^[-\d. e]+$/);
  });
});
