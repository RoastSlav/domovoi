import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ComponentType,
  type FocusEvent,
  type Ref,
} from "react";
import { GRAPH_INSTRUCTIONS, graphLabel, liveLine } from "./announce";
import { GraphLegend } from "./GraphLegend";
import { GraphToolbar } from "./GraphToolbar";
import { planTransitions, ANIMATE_LAYOUT_MAX_NODES } from "./transitions";
import { entryNode, handleGraphKey, type KeyContext } from "./useGraphKeys";
import styles from "./graph.module.css";
import type {
  GraphEngineHandle,
  GraphEngineProps,
  GraphModel,
  GraphNodeModel,
  NodePosition,
  StepGraphProps,
} from "./types";

/** What the shell is given besides the caller's props: the engine and where to report failure. */
export interface ShellProps extends StepGraphProps {
  Engine: ComponentType<GraphEngineProps>;
  onFailure(message: string): void;
}

interface Plan {
  model: GraphModel;
  rememberKey: string;
  remembered: ReadonlySet<string>;
  animate: ReadonlySet<string>;
}

const NO_TRANSITIONS: ReadonlySet<string> = new Set();

/** The sentence for a node, with how many steps it depends on and is needed by. */
function lineFor(model: GraphModel, node: GraphNodeModel, selectedId: string | null): string {
  const dependsOn = new Set(model.edges.filter((e) => e.from === node.id).map((e) => e.to));
  const neededBy = new Set(model.edges.filter((e) => e.to === node.id).map((e) => e.from));
  return liveLine(node, {
    dependsOn: dependsOn.size,
    neededBy: neededBy.size,
    selected: node.id === selectedId,
  });
}

/**
 * The library-free half of the step graph: one tab stop, the keyboard model, the spoken
 * line, the toolbar, the edge key and the test seams. Focus is not selection: arrows move
 * focus, Enter and clicks select.
 */
export function GraphShell(props: ShellProps) {
  const {
    model,
    selectedId,
    Engine,
    rememberKey = "live",
    animate = true,
    openerRef,
    label,
    onSelect,
    onOpen,
    onClear,
    onToggleGroup,
    onFoldAll,
    onFailure,
  } = props;
  const id = useId();
  const handle = useRef<GraphEngineHandle>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [rootFocused, setRootFocused] = useState(false);
  const [layout, setLayout] = useState<{
    key: string;
    centres: ReadonlyMap<string, NodePosition>;
  } | null>(null);
  const [line, setLine] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [seenSelected, setSeenSelected] = useState(selectedId);
  const [plan, setPlan] = useState<Plan>(() => ({
    model,
    rememberKey,
    remembered: planTransitions(null, model, new Set()).remembered,
    animate: NO_TRANSITIONS,
  }));

  // Derived state, settled during render so the engine never draws a stale frame.
  let transitions = plan.animate;
  if (plan.model !== model || plan.rememberKey !== rememberKey) {
    const reset = plan.rememberKey !== rememberKey;
    const next = planTransitions(plan.model, model, reset ? NO_TRANSITIONS : plan.remembered);
    transitions = animate ? next.animate : NO_TRANSITIONS;
    setPlan({ model, rememberKey, remembered: next.remembered, animate: transitions });
  }
  const nodeById = new Map(model.nodes.map((node) => [node.id, node]));
  const say = (node: GraphNodeModel, selected: string | null): void =>
    setLine(lineFor(model, node, selected));
  if (seenSelected !== selectedId) {
    setSeenSelected(selectedId);
    const target = selectedId === null ? undefined : nodeById.get(selectedId);
    if (target) {
      if (!rootFocused) {
        setFocusedId(target.id);
      }
      say(target, selectedId);
    }
  }
  if (pending !== null && nodeById.has(pending)) {
    setFocusedId(pending);
    setPending(null);
    say(nodeById.get(pending) as GraphNodeModel, selectedId);
  } else if (focusedId !== null && !nodeById.has(focusedId)) {
    const group = model.nodes.find((node) => node.members.includes(focusedId));
    setFocusedId(group?.id ?? null);
  }

  const latest = useRef({ rootFocused, model });
  useEffect(() => {
    latest.current = { rootFocused, model };
  });
  const layoutKey = layout?.key;
  useEffect(() => {
    const { rootFocused: inside, model: now } = latest.current;
    if (selectedId !== null && !inside && now.nodes.some((node) => node.id === selectedId)) {
      handle.current?.reveal(selectedId);
    }
  }, [selectedId, layoutKey]);

  const onLayout = useCallback((positions: readonly NodePosition[], key: string) => {
    setLayout({ key, centres: new Map(positions.map((p) => [p.id, p])) });
  }, []);
  const centres = layout?.centres ?? new Map<string, NodePosition>();
  const focusNode = (target: string): void => {
    const node = nodeById.get(target);
    if (!node) {
      return;
    }
    setFocusedId(target);
    say(node, selectedId);
    handle.current?.reveal(target);
  };
  const toggle = (node: GraphNodeModel): void => {
    setPending(node.members[0] ?? null);
    onToggleGroup(node.id);
  };
  const context: KeyContext = {
    model,
    centres,
    focusedId,
    selectedId,
    handle,
    focus: focusNode,
    open: (node) => (node.kind === "group" ? toggle(node) : onOpen(node.id)),
    clear: onClear,
  };
  const onFocus = (event: FocusEvent<HTMLElement>): void => {
    setRootFocused(true);
    if (event.target !== event.currentTarget) {
      return;
    }
    const entry = entryNode(context);
    if (entry !== null) {
      focusNode(entry);
    }
  };
  const pick = (target: string): void => {
    const node = nodeById.get(target);
    if (!node) {
      return;
    }
    focusNode(target);
    if (node.kind === "group") {
      toggle(node);
    } else {
      onSelect(target);
    }
  };
  const open = (target: string): void => {
    const node = nodeById.get(target);
    if (node?.kind === "step") {
      onOpen(target);
    }
  };

  const ready = layout !== null && layout.key === model.layoutKey;
  const groups = model.nodes.filter((node) => node.kind === "group");
  return (
    <div data-graph-frame="" className={styles.frame}>
      <GraphToolbar
        onFit={() => handle.current?.fit()}
        onZoom={(factor) => handle.current?.zoomBy(factor)}
        onFoldAll={onFoldAll}
      />
      <div
        ref={openerRef as Ref<HTMLDivElement> | undefined}
        className={styles.canvas}
        data-graph=""
        role="group"
        aria-roledescription="graph"
        aria-label={graphLabel(label)}
        aria-describedby={`${id}-help`}
        tabIndex={0}
        data-variant="svg-dagre"
        data-ready={ready ? "true" : "false"}
        data-focused-node={focusedId ?? ""}
        data-selected-step={selectedId ?? ""}
        data-graph-nodes={model.nodes.map((node) => node.id).join(" ")}
        data-graph-groups={groups.map((node) => node.id).join(" ")}
        data-layout-key={model.layoutKey}
        onKeyDown={(event) => handleGraphKey(event, context)}
        onFocus={onFocus}
        onBlur={() => setRootFocused(false)}
      >
        <Engine
          ref={handle}
          model={model}
          focusedId={focusedId}
          selectedId={selectedId}
          rootFocused={rootFocused}
          transitions={transitions}
          animateLayout={animate && model.nodes.length <= ANIMATE_LAYOUT_MAX_NODES}
          onPointerSelect={pick}
          onPointerOpen={open}
          onBackground={onClear}
          onLayout={onLayout}
          onFailure={onFailure}
        />
      </div>
      <p id={`${id}-help`} className="visually-hidden">
        {GRAPH_INSTRUCTIONS}
      </p>
      <div role="status" aria-live="polite" className="visually-hidden">
        {ready && model.nodes.length > 0 ? line : ""}
      </div>
      <GraphLegend edges={model.edges} />
    </div>
  );
}
