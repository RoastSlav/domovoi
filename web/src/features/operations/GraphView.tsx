import { useRef } from "react";
import type { OperationTrace } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { useRightSlot } from "../../app/rightSlot";
import { Button, Cluster, Stack, Text, Visible } from "../../components";
import { OperationGraph } from "./graph/OperationGraph";
import { ReplayBar } from "./replay/ReplayBar";
import { ReplayFrame } from "./replay/ReplayFrame";
import { REPLAY_TEXT } from "./replay/replayText";
import { useGraphReplay } from "./replay/useGraphReplay";
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
  const replay = useGraphReplay(trace);
  const shown = replay.playback.replayTrace ?? trace;
  const selectedInShown =
    selected && shown.steps.some((step) => step.id === selected) ? selected : null;
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
        <Stack gap={3}>
          <Cluster justify="end">
            <Visible category="P1" id="operation.replay-toggle">
              <Button variant="secondary" icon="play" onClick={replay.toggle}>
                {replay.open ? REPLAY_TEXT.close : REPLAY_TEXT.open}
              </Button>
            </Visible>
          </Cluster>
          {replay.open && (
            <ReplayBar
              playback={replay.playback}
              trace={trace}
              load={replay.load}
              reason={replay.history?.incomplete_reason}
            />
          )}
          <ReplayFrame
            playback={replay.playback}
            open={replay.open}
            replaying={replay.playback.replayTrace !== undefined}
          >
            <OperationGraph
              trace={shown}
              selected={selectedInShown}
              onSelect={(id) => onSelect(id, root.current ?? document.body)}
              onOpen={open}
              onClear={onClear}
              openerRef={root}
              rememberKey={String(replay.playback.state.session)}
              emptyState={trace.steps.length === 0}
            />
          </ReplayFrame>
        </Stack>
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
