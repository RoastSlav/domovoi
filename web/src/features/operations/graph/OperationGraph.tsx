import { useEffect, useMemo } from "react";
import type { OperationTrace } from "../../../api/operationTrace";
import {
  Button,
  EmptyState,
  StepGraph,
  Stack,
  preloadGraph,
  useProfile,
  type StepGraphProps,
} from "../../../components";
import { buildGraphModel } from "./graphModel";
import { useGraphState } from "./useGraphState";

/** What the graph of an operation needs; a later change may pass a reconstructed past state. */
export interface OperationGraphProps {
  /** The trace to draw: today the live one. */
  trace: OperationTrace;
  /** A step id. */
  selected: string | null;
  /** Click or tap on a step. */
  onSelect(id: string): void;
  /** Enter or double-click on a step. */
  onOpen(id: string): void;
  onClear(): void;
  /** Default true. */
  animate?: boolean;
  /** Default "live"; passed through to the graph. */
  rememberKey?: string;
  /** Default "Step graph". */
  label?: string;
  /** Receives the graph's focusable root, so the inspector can return focus to it. */
  openerRef?: StepGraphProps["openerRef"];
}

/**
 * The graph of an operation's steps: builds the model from the trace and the detail level
 * (technical steps fold in Standard and Essential, problems never fold), and keeps which groups
 * are open. The selection belongs to the caller (the address).
 */
export function OperationGraph({
  trace,
  selected,
  onSelect,
  onOpen,
  onClear,
  animate,
  rememberKey,
  label,
  openerRef,
}: OperationGraphProps) {
  const { profile } = useProfile();
  const { expanded, showAll, setShowAll, toggleGroup, foldAll } = useGraphState();
  useEffect(() => {
    void preloadGraph();
  }, []);
  const built = useMemo(
    () => buildGraphModel({ trace, profile, expanded, selected, showAll }),
    [trace, profile, expanded, selected, showAll],
  );
  if (trace.steps.length === 0) {
    return <EmptyState icon="play" title="No steps yet" />;
  }
  return (
    <Stack gap={3}>
      {built.hiddenEarlier > 0 && (
        <div>
          <Button variant="secondary" onClick={() => setShowAll(true)}>
            {`Show ${built.hiddenEarlier} earlier steps`}
          </Button>
        </div>
      )}
      <StepGraph
        model={built.model}
        selectedId={selected}
        onSelect={onSelect}
        onOpen={onOpen}
        onClear={onClear}
        onToggleGroup={toggleGroup}
        {...(expanded.size > 0 ? { onFoldAll: foldAll } : {})}
        {...(animate !== undefined ? { animate } : {})}
        {...(rememberKey !== undefined ? { rememberKey } : {})}
        {...(label !== undefined ? { label } : {})}
        {...(openerRef !== undefined ? { openerRef } : {})}
      />
    </Stack>
  );
}
