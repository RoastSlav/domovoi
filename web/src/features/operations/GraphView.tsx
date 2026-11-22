import { useRef } from "react";
import type { OperationTrace } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { useRightSlot } from "../../app/rightSlot";
import { Stack, Text, Visible } from "../../components";
import { OperationGraph } from "./graph/OperationGraph";
import { StepOutline } from "./StepOutline";
import { INSPECTOR_ID } from "./useStepSelection";

/**
 * The Graph view: the graph of the steps, then the same steps as a list, which is the screen
 * reader's path and the fallback when the graph cannot be drawn. Arrow keys in the graph move
 * focus only; a click, Enter or double-click selects a step and opens the inspector.
 */
export function GraphView({
  trace,
  executions,
  selected,
  onSelect,
  onClear,
}: {
  trace: OperationTrace;
  executions: readonly Execution[];
  selected: string | undefined;
  onSelect: (id: string, opener: HTMLElement) => void;
  onClear: () => void;
}) {
  const root = useRef<HTMLElement | null>(null);
  const slot = useRightSlot();
  const open = (id: string) => {
    if (selected === id) {
      slot.focus(INSPECTOR_ID);
    } else {
      onSelect(id, root.current ?? document.body);
    }
  };
  return (
    <Stack gap={3}>
      <Visible category="P1" id="operation.graph">
        <OperationGraph
          trace={trace}
          selected={selected ?? null}
          onSelect={(id) => onSelect(id, root.current ?? document.body)}
          onOpen={open}
          onClear={onClear}
          openerRef={root}
        />
      </Visible>
      <Text muted block>
        The same steps as a list:
      </Text>
      <StepOutline
        trace={trace}
        executions={executions}
        selected={selected}
        onSelect={onSelect}
        view="timeline"
      />
    </Stack>
  );
}
