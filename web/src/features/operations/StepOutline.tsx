import { useState } from "react";
import type { OperationTrace, TraceStep } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { Button, IconButton, Text } from "../../components";
import { buildTree, byCycle, retryTarget, windowSteps, type StepNode } from "./steps";
import { StepRow } from "./StepRow";
import styles from "./operations.module.css";

/** How many of the newest steps show before "Show earlier steps". */
export const STEP_LIMIT = 200;

/** Which grouping the outline uses. */
export type OutlineView = "timeline" | "tree";

interface Shared {
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
      execution={shared.byStep.get(step.id)}
      selected={shared.selected === step.id}
      retry={retryTarget(shared.trace, step.id) !== undefined}
      onSelect={shared.onSelect}
    />
  );
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
          {open && (
            <ol className={styles.steps} aria-label="Steps under this one">
              {node.children.map((child) => (
                <Branch key={child.step.id} node={child} shared={shared} />
              ))}
            </ol>
          )}
        </li>
      )}
    </>
  );
}

/**
 * The trace's steps as a timeline (one list per cycle) or a tree (grouped by parent). Long
 * traces show the newest steps, every open one and the selected one; the rest are one click away.
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
  const shown = windowSteps(trace.steps, {
    limit: STEP_LIMIT,
    showAll,
    keep: selected ? [selected] : [],
  });
  const hidden = trace.steps.length - shown.length;
  const shared: Shared = {
    trace,
    byStep: new Map(executions.map((execution) => [execution.step, execution])),
    selected,
    onSelect,
  };
  const cycles = byCycle(shown);
  return (
    <div className={styles.outline}>
      {hidden > 0 && (
        <Button size="sm" onClick={() => setShowAll(true)}>
          {`Show ${hidden} earlier steps`}
        </Button>
      )}
      {view === "tree" ? (
        <ol className={styles.steps} aria-label="Steps">
          {buildTree(shown).map((node) => (
            <Branch key={node.step.id} node={node} shared={shared} />
          ))}
        </ol>
      ) : (
        cycles.map((group) => (
          <div key={group.cycle}>
            {cycles.length > 1 && (
              <Text muted block size="sm">
                Cycle {group.cycle}
              </Text>
            )}
            <ol
              className={styles.steps}
              aria-label={cycles.length > 1 ? `Cycle ${group.cycle}` : "Steps"}
            >
              {group.steps.map((step) => row(step, shared))}
            </ol>
          </div>
        ))
      )}
    </div>
  );
}
