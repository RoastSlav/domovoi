import { useMemo, useState } from "react";
import type { OperationTrace, TraceStep } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { Button, IconButton, Text, Visible, useProfile } from "../../components";
import type { Category } from "../../design/visibility";
import { buildTree, byCycle, retryTarget, windowSteps, type StepNode } from "./steps";
import { StepGroup } from "./StepGroup";
import { StepRow } from "./StepRow";
import { groupSteps, stepCategory, treeCategory } from "./stepVisibility";
import styles from "./operations.module.css";

/** How many of the newest steps show before "Show earlier steps". */
export const STEP_LIMIT = 200;

/** Which grouping the outline uses. */
export type OutlineView = "timeline" | "tree";

interface Shared {
  categories: Map<string, Category>;
  trace: OperationTrace;
  byStep: Map<string, Execution>;
  selected: string | undefined;
  onSelect: (id: string, opener: HTMLElement) => void;
}

function row(step: TraceStep, shared: Shared) {
  return (
    <StepRow
      key={step.id}
      step={step}
      category={shared.categories.get(step.id) ?? stepCategory(step.kind, step.outcome)}
      execution={shared.byStep.get(step.id)}
      selected={shared.selected === step.id}
      retry={retryTarget(shared.trace, step.id) !== undefined}
      onSelect={shared.onSelect}
    />
  );
}

function holds(node: StepNode, id: string | undefined): boolean {
  return id !== undefined && (node.step.id === id || node.children.some((c) => holds(c, id)));
}

function Branch({ node, shared }: { node: StepNode; shared: Shared }) {
  const [open, setOpen] = useState(true);
  return (
    <>
      {row(node.step, shared)}
      {node.children.length > 0 && (
        <li className={styles.branch}>
          <IconButton
            icon={open ? "chevron-down" : "chevron-right"}
            label={open ? "Fold steps under this one" : "Unfold steps under this one"}
            size="sm"
            aria-expanded={open}
            onClick={() => setOpen((now) => !now)}
          />
          {open && <Siblings nodes={node.children} shared={shared} label="Steps under this one" />}
        </li>
      )}
    </>
  );
}

/** A list of sibling nodes, with runs of technical ones folded when the detail level says so. */
function Siblings({ nodes, shared, label }: { label: string; nodes: StepNode[]; shared: Shared }) {
  const { profile } = useProfile();
  const items = groupSteps(nodes, profile, (node) => shared.categories.get(node.step.id) ?? "P1");
  return (
    <ol className={styles.steps} aria-label={label}>
      {items.map((entry) =>
        entry.type === "item" ? (
          <Branch key={entry.item.step.id} node={entry.item} shared={shared} />
        ) : (
          <StepGroup
            key={entry.items[0]?.step.id}
            steps={entry.items.map((node) => node.step)}
            reveal={entry.items.some((node) => holds(node, shared.selected))}
          >
            {entry.items.map((node) => (
              <Branch key={node.step.id} node={node} shared={shared} />
            ))}
          </StepGroup>
        ),
      )}
    </ol>
  );
}

/**
 * The trace's steps as a timeline (one list per cycle) or a tree (grouped by parent). Long
 * traces show the newest steps, every open one and the selected one; the rest are one click away.
 * Runs of technical steps fold into one toggle when the detail level asks for it.
 */
export function StepOutline({
  trace,
  executions,
  selected,
  onSelect,
  view,
}: {
  trace: OperationTrace;
  executions: readonly Execution[];
  selected: string | undefined;
  onSelect: (id: string, opener: HTMLElement) => void;
  view: OutlineView;
}) {
  const [showAll, setShowAll] = useState(false);
  const { profile } = useProfile();
  const shown = windowSteps(trace.steps, {
    limit: STEP_LIMIT,
    showAll,
    keep: selected ? [selected] : [],
  });
  const hidden = trace.steps.length - shown.length;
  const categories = useMemo(
    () =>
      new Map(
        shown.map((step) => [
          step.id,
          view === "tree" ? treeCategory(shown, step.id) : stepCategory(step.kind, step.outcome),
        ]),
      ),
    [shown, view],
  );
  const shared: Shared = {
    categories,
    trace,
    byStep: new Map(executions.map((execution) => [execution.step, execution])),
    selected,
    onSelect,
  };
  const cycles = byCycle(shown);
  return (
    <div className={styles.outline}>
      {hidden > 0 && (
        <Visible category="P1" id="operation.earlier-steps">
          <Button size="sm" onClick={() => setShowAll(true)}>
            {`Show ${hidden} earlier steps`}
          </Button>
        </Visible>
      )}
      {view === "tree" ? (
        <Siblings nodes={buildTree(shown)} shared={shared} label="Steps" />
      ) : (
        cycles.map((group) => {
          const items = groupSteps(group.steps, profile, (step) => categories.get(step.id) ?? "P1");
          return (
            <div key={group.cycle}>
              {cycles.length > 1 && (
                <Visible category="P1" id="operation.cycle-label">
                  <Text muted block size="sm">
                    Cycle {group.cycle}
                  </Text>
                </Visible>
              )}
              <ol
                className={styles.steps}
                aria-label={cycles.length > 1 ? `Cycle ${group.cycle}` : "Steps"}
              >
                {items.map((entry) =>
                  entry.type === "item" ? (
                    row(entry.item, shared)
                  ) : (
                    <StepGroup
                      key={entry.items[0]?.id}
                      steps={entry.items}
                      reveal={entry.items.some((step) => step.id === selected)}
                    >
                      {entry.items.map((step) => row(step, shared))}
                    </StepGroup>
                  ),
                )}
              </ol>
            </div>
          );
        })
      )}
    </div>
  );
}
