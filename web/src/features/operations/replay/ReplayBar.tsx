import type { OperationTrace } from "../../../api/operationTrace";
import {
  Button,
  Cluster,
  IconButton,
  InlineMessage,
  LoadError,
  Scrubber,
  SegmentedControl,
  Skeleton,
  Stack,
  Text,
  Visible,
} from "../../../components";
import { formatClock } from "./clock";
import { frameCaption } from "./frames";
import type { Speed } from "./playback";
import { momentLine, momentText, noteTitle, REPLAY_TEXT } from "./replayText";
import type { Playback } from "./usePlayback";

const SPEEDS = [
  { value: "1", label: "1×" },
  { value: "2", label: "2×" },
  { value: "4", label: "4×" },
] as const;

/** Where the history stands: still loading, failed (with the error), or in hand. */
export type HistoryLoad =
  | { kind: "loading" }
  | { kind: "failed"; error: unknown; retry: () => unknown }
  | { kind: "ready" };

/** The notes about records that are missing: the oldest removed, the latest not read. */
function Gaps({ reason }: { reason: string | null | undefined }) {
  const pruned = reason === "pruned" || reason === "both";
  const capped = reason === "capped" || reason === "both";
  if (!pruned && !capped) {
    return null;
  }
  return (
    <Visible category="P0" id="operation.replay-gap">
      <Stack gap={1}>
        {pruned && <Text muted>{REPLAY_TEXT.pruned}</Text>}
        {capped && <Text muted>{REPLAY_TEXT.capped}</Text>}
      </Stack>
    </Visible>
  );
}

/**
 * The replay controls: Previous, Play or Pause, Next, speed, Back to live, the position
 * slider and a line saying where it stands. It only moves the graph's moment; it decides nothing.
 */
export function ReplayBar({
  playback,
  trace,
  load,
  reason,
}: {
  load: HistoryLoad;
  playback: Playback;
  /** Why the history is incomplete: pruned, capped, both, or none. */
  reason: string | null | undefined;
  trace: OperationTrace;
}) {
  if (load.kind === "loading") {
    return (
      <Visible category="P1" id="operation.replay-bar">
        <Skeleton shape="block" />
      </Visible>
    );
  }
  if (load.kind === "failed") {
    return (
      <Visible category="P1" id="operation.replay-bar">
        <Visible category="P0" id="operation.replay-gap">
          <LoadError error={load.error} context={REPLAY_TEXT.loadWhat} onRetry={load.retry} />
        </Visible>
      </Visible>
    );
  }
  const { positions, index, state, frame, reduced } = playback;
  const atLive = state.position.kind === "live";
  const count = positions.length - 1;
  const clock = frame ? formatClock(frame.atMs) : "";
  const backToLive = (
    <Button
      variant="secondary"
      onClick={playback.live}
      {...(atLive ? { disabledReason: REPLAY_TEXT.atLive } : {})}
    >
      {REPLAY_TEXT.live}
    </Button>
  );
  if (positions.length < 2) {
    return (
      <Visible category="P1" id="operation.replay-bar">
        <Stack gap={2}>
          <div role="group" aria-label={REPLAY_TEXT.group}>
            <Cluster justify="between">
              <Text muted>{REPLAY_TEXT.nothing}</Text>
              {backToLive}
            </Cluster>
          </div>
          <Gaps reason={reason} />
        </Stack>
      </Visible>
    );
  }
  const playReason = reduced ? REPLAY_TEXT.reduced : undefined;
  const line = atLive
    ? REPLAY_TEXT.liveLine
    : momentLine(index, count, clock, frame ? frameCaption(frame, trace) : "Recorded");
  return (
    <Visible category="P1" id="operation.replay-bar">
      <Stack gap={2}>
        <div role="group" aria-label={REPLAY_TEXT.group}>
          <Stack gap={2}>
            <Cluster justify="between">
              <Cluster>
                <IconButton
                  icon="chevron-left"
                  label={REPLAY_TEXT.previous}
                  onClick={playback.previous}
                  {...(index === 0 ? { disabledReason: REPLAY_TEXT.atFirst } : {})}
                />
                {state.playing ? (
                  <Button icon="pause" onClick={playback.pause}>
                    {REPLAY_TEXT.pause}
                  </Button>
                ) : (
                  <Button
                    icon="play"
                    onClick={playback.play}
                    {...(playReason ? { disabledReason: playReason } : {})}
                  >
                    {REPLAY_TEXT.play}
                  </Button>
                )}
                <IconButton
                  icon="chevron-right"
                  label={REPLAY_TEXT.next}
                  onClick={playback.next}
                  {...(atLive ? { disabledReason: REPLAY_TEXT.atLive } : {})}
                />
                <SegmentedControl
                  label={REPLAY_TEXT.speed}
                  options={SPEEDS}
                  value={String(state.speed) as "1" | "2" | "4"}
                  onChange={(value) => playback.setSpeed(Number(value) as Speed)}
                />
              </Cluster>
              {backToLive}
            </Cluster>
            <Scrubber
              label={REPLAY_TEXT.scrubber}
              min={0}
              max={count}
              value={index}
              onChange={playback.seek}
              valueText={atLive ? REPLAY_TEXT.liveLine : momentText(index, count, clock)}
            />
            <p role="status" aria-live={state.playing ? "off" : "polite"}>
              {line}
            </p>
          </Stack>
        </div>
        <Gaps reason={reason} />
        {!atLive && (
          <Visible category="P0" id="operation.replay-note">
            <InlineMessage tone="info" title={noteTitle(clock)}>
              {REPLAY_TEXT.noteBody}
            </InlineMessage>
          </Visible>
        )}
      </Stack>
    </Visible>
  );
}
