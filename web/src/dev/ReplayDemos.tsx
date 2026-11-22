import { useEffect, useState } from "react";
import { Scrubber, Stack, Text } from "../components";
import { ApiFailure } from "../api/types";
import type { OperationHistory } from "../api/types";
import { ReplayBar, type HistoryLoad } from "../features/operations/replay/ReplayBar";
import { usePlayback } from "../features/operations/replay/usePlayback";
import { historyFor, historyOf, ev, must, TRACE_FIXTURES } from "../test/replayFixtures";
import { Demo, Group } from "./Demo";

const TRACE = must(TRACE_FIXTURES["verified-success"]);
const HISTORY = historyFor(TRACE);
const READY: HistoryLoad = { kind: "ready" };
const FAILED: HistoryLoad = {
  kind: "failed",
  error: new ApiFailure(500, "internal", "The hub could not read the history."),
  retry: () => undefined,
};
const ONE_MOMENT = historyOf([ev(1, "2026-01-01T00:00:00Z", "operation.created")]);

/** The bar over a real playback of the fixture, put in one state for the catalogue. */
function Bar({
  history = HISTORY,
  load = READY,
  startAt,
  playing = false,
  reduced = false,
}: {
  history?: OperationHistory;
  load?: HistoryLoad;
  playing?: boolean;
  reduced?: boolean;
  /** A position index to start at; live when left out. */
  startAt?: number;
}) {
  const playback = usePlayback(TRACE, history);
  const { seek } = playback;
  useEffect(() => {
    if (startAt !== undefined) {
      seek(startAt);
    }
  }, [seek, startAt]);
  // The catalogue shows a state; nothing runs, so playing and reduced motion are only labels here.
  const shown = { ...playback, reduced, state: { ...playback.state, playing } };
  return (
    <ReplayBar playback={shown} trace={TRACE} load={load} reason={history.incomplete_reason} />
  );
}

/** A scrubber over seven positions, with its own value. */
function ScrubberDemo({ disabled = false }: { disabled?: boolean }) {
  const [value, setValue] = useState(3);
  return (
    <Scrubber
      label={disabled ? "Position (off)" : "Position"}
      min={0}
      max={6}
      value={value}
      onChange={setValue}
      valueText={`Position ${value + 1} of 7`}
      disabled={disabled}
      {...(disabled ? { disabledReason: "Nothing to move through yet." } : {})}
    />
  );
}

/** `/dev/design`: the Scrubber and the replay bar in each of its states. */
export function ReplayDemos() {
  return (
    <Group title="Replay">
      <Demo name="Scrubber">
        <Stack gap={4}>
          <ScrubberDemo />
          <ScrubberDemo disabled />
          <Text muted>Arrow keys move by one, Page keys by a tenth, Home and End to the ends.</Text>
        </Stack>
      </Demo>
      <Demo name="Replay bar, at live">
        <Bar />
      </Demo>
      <Demo name="Replay bar, replaying">
        <Bar startAt={2} />
      </Demo>
      <Demo name="Replay bar, playing">
        <Bar startAt={1} playing />
      </Demo>
      <Demo name="Replay bar, reduced motion">
        <Bar reduced />
      </Demo>
      <Demo name="Replay bar, loading the history">
        <Bar load={{ kind: "loading" }} />
      </Demo>
      <Demo name="Replay bar, history failed">
        <Bar load={FAILED} />
      </Demo>
      <Demo name="Replay bar, oldest records removed">
        <Bar history={{ ...HISTORY, complete: false, incomplete_reason: "pruned" }} />
      </Demo>
      <Demo name="Replay bar, latest records missing">
        <Bar history={{ ...HISTORY, complete: false, incomplete_reason: "capped" }} />
      </Demo>
      <Demo name="Replay bar, nothing to replay">
        <Bar history={ONE_MOMENT} />
      </Demo>
    </Group>
  );
}
