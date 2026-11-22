import { useState } from "react";
import type { OperationTrace } from "../api/operationTrace";
import { Button, Cluster, InlineMessage, Stack, Text } from "../components";
import type { ProfileId } from "../design/visibility";
import { OperationGraph } from "../features/operations/graph/OperationGraph";
import { TRACE_FIXTURES, type TraceFixtureName } from "../test/operationFixtures";
import { ProfileProvider } from "../test/visibility";
import { Demo, Group } from "./Demo";

const NAMES = Object.keys(TRACE_FIXTURES) as TraceFixtureName[];

/** A graph with its own selection, as the operation page keeps it in the address. */
function Drawn({
  trace,
  profile,
  label,
}: {
  label: string;
  profile: ProfileId;
  trace: OperationTrace;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  return (
    <ProfileProvider profile={profile}>
      <OperationGraph
        trace={trace}
        selected={selected}
        onSelect={setSelected}
        onOpen={setSelected}
        onClear={() => setSelected(null)}
        rememberKey={label}
        label={label}
      />
    </ProfileProvider>
  );
}

/** The message the graph shows when its engine cannot load or throws, with a flag to see it. */
function Fallback() {
  const [failed, setFailed] = useState(false);
  return (
    <Stack gap={3}>
      <Cluster>
        <Button size="sm" aria-pressed={failed} onClick={() => setFailed((now) => !now)}>
          {failed ? "Show the graph again" : "Show the failure"}
        </Button>
      </Cluster>
      {failed ? (
        <InlineMessage
          tone="warning"
          title="The graph could not be drawn"
          action={<Button size="sm">Try again</Button>}
        >
          The step list below has the same steps.
        </InlineMessage>
      ) : (
        <Text muted>The graph is drawn as usual.</Text>
      )}
    </Stack>
  );
}

/** The step graph on the five golden traces, at two detail levels, with its failure message. */
export function GraphDemos() {
  const [name, setName] = useState<TraceFixtureName>("waiting-approval");
  const trace = TRACE_FIXTURES[name] as unknown as OperationTrace;
  return (
    <Group title="Step graph">
      <Cluster>
        {NAMES.map((one) => (
          <Button
            key={one}
            size="sm"
            variant={one === name ? "primary" : "secondary"}
            aria-pressed={one === name}
            onClick={() => setName(one)}
          >
            {one}
          </Button>
        ))}
      </Cluster>
      <Demo name="Detailed: every step drawn">
        <Drawn trace={trace} profile="detailed" label={`Detailed graph of ${name}`} />
      </Demo>
      <Demo name="Standard: technical steps folded">
        <Drawn trace={trace} profile="standard" label={`Standard graph of ${name}`} />
      </Demo>
      <Demo name="Every kind of connection, with the edge key">
        <Drawn
          trace={TRACE_FIXTURES["failed-verification-new-cycle"] as unknown as OperationTrace}
          profile="detailed"
          label="Graph with every kind of connection"
        />
      </Demo>
      <Demo name="When the graph cannot be drawn">
        <Fallback />
      </Demo>
    </Group>
  );
}
