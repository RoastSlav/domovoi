// Test support only: a stand-in for the real engine that tests mount through vi.mock.
import { useEffect, useImperativeHandle, useLayoutEffect, useRef } from "react";
import { vi } from "vitest";
import type { GraphEngineProps, GraphModel, NodePosition } from "./types";

const COLUMN = 300;
const ROW = 100;

/** What the test engine recorded; reset it in `beforeEach`. */
export const engineSpy = {
  /** The props of the latest render. */
  props: null as GraphEngineProps | null,
  /** Every `transitions` set the engine was given, in render order. */
  transitions: [] as ReadonlySet<string>[],
  mounts: 0,
  /** Report this failure from the next mounted engine. */
  failure: null as string | null,
  handle: { fit: vi.fn(), zoomBy: vi.fn(), reveal: vi.fn() },
};

/** Clears the recorded calls and the failure switches. */
export function resetEngine(): void {
  engineSpy.props = null;
  engineSpy.transitions = [];
  engineSpy.mounts = 0;
  engineSpy.failure = null;
  engineSpy.handle.fit.mockClear();
  engineSpy.handle.zoomBy.mockClear();
  engineSpy.handle.reveal.mockClear();
}

/** Centres on a left-to-right grid: the column is the longest path from a root, rows follow ordinal order. */
export function gridPositions(model: GraphModel): NodePosition[] {
  const rank = new Map<string, number>();
  for (const node of model.nodes) {
    const needs = model.edges.filter((e) => e.from === node.id).map((e) => rank.get(e.to) ?? 0);
    rank.set(node.id, needs.length > 0 ? Math.max(...needs) + 1 : 0);
  }
  const rows = new Map<number, number>();
  return model.nodes.map((node) => {
    const column = rank.get(node.id) ?? 0;
    const row = rows.get(column) ?? 0;
    rows.set(column, row + 1);
    return { id: node.id, x: column * COLUMN, y: row * ROW };
  });
}

/** Draws each node as a plain element and answers the handle with spies. */
export default function TestEngine(props: GraphEngineProps) {
  const { model, selectedId, focusedId, rootFocused, ref } = props;
  const latest = useRef(props);
  useLayoutEffect(() => {
    latest.current = props;
    engineSpy.props = props;
    engineSpy.transitions.push(props.transitions);
  });
  useImperativeHandle(
    ref,
    () => ({ ...engineSpy.handle, positions: () => gridPositions(latest.current.model) }),
    [],
  );
  useEffect(() => {
    engineSpy.mounts += 1;
    if (engineSpy.failure !== null) {
      latest.current.onFailure(engineSpy.failure);
    }
  }, []);
  useEffect(() => {
    const { model, onLayout } = latest.current;
    onLayout(gridPositions(model), model.layoutKey);
  }, [model.layoutKey]);
  return (
    <div data-test-engine="">
      {model.nodes.map((node) => (
        <div
          key={node.id}
          data-node-id={node.id}
          data-selected={node.id === selectedId ? "true" : undefined}
          data-focused={rootFocused && node.id === focusedId ? "true" : undefined}
        >
          {node.title} {node.status.label} {node.summary}
        </div>
      ))}
    </div>
  );
}
