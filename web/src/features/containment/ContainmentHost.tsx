import { Link } from "react-router";
import { useHostContainment, useRefreshContainment } from "../../api/hooks/containment";
import type { ContainmentBlock, ContainmentStatus } from "../../api/types";
import {
  Button,
  Cluster,
  QueryBoundary,
  Skeleton,
  Stack,
  StatusPill,
  Text,
  Visible,
  useActionFeedback,
} from "../../components";
import {
  BRIDGE_GAP_TEXT,
  LOST_WHY,
  NOT_ASKED,
  existingFlowsText,
  gateNote,
} from "../../design/containmentText";
import { blockPhaseMeta, drillsStateMeta, gateStateMeta } from "../../design/tone";

function BlockRow({ block }: { block: ContainmentBlock }) {
  return (
    <li>
      <Cluster>
        <code>{block.source}</code>
        <StatusPill size="sm" meta={blockPhaseMeta(block.phase)} />
        <Link to={`/operations/${block.operation_id}`}>Open operation</Link>
      </Cluster>
    </li>
  );
}

function Details({ status, host }: { host: string; status: ContainmentStatus }) {
  const refresh = useActionFeedback(useRefreshContainment(host), {
    success: "Asked the helper which blocks it holds.",
    error: "refresh the blocks",
  });
  const flows = existingFlowsText(status.existing_flows);
  return (
    <Stack gap={2}>
      <Visible category="P0" id="containment.gate">
        {status.gate.state === "unknown" ? (
          <Text block size="sm">
            {NOT_ASKED}
          </Text>
        ) : (
          <Cluster>
            <StatusPill meta={gateStateMeta(status.gate.state)} />
            {status.drills.state !== "unknown" && (
              <StatusPill meta={drillsStateMeta(status.drills.state)} />
            )}
          </Cluster>
        )}
        {gateNote(status.gate.state) !== null && (
          <Text block size="sm">
            {gateNote(status.gate.state)}
          </Text>
        )}
        {status.lost_blocks && (
          <Text block size="sm">
            {LOST_WHY}
          </Text>
        )}
      </Visible>
      <Visible category="P1" id="containment.blocks">
        {status.blocks.length === 0 ? (
          <Text muted block size="sm">
            No blocks recorded on this host.
          </Text>
        ) : (
          <ul aria-label={`Blocks on ${host}`}>
            {status.blocks.map((block) => (
              <BlockRow key={block.block_id} block={block} />
            ))}
          </ul>
        )}
      </Visible>
      <Visible category="P2" id="containment.notes" label="Flows and bridges">
        <Stack gap={1}>
          {flows !== null && (
            <Text block size="sm">
              {flows}
            </Text>
          )}
          {status.bridge_gap === true && (
            <Text block size="sm">
              {BRIDGE_GAP_TEXT}
            </Text>
          )}
        </Stack>
      </Visible>
      <Button
        size="sm"
        variant="secondary"
        icon="refresh"
        loading={refresh.isPending}
        disabledReason={
          status.available ? undefined : "The helper on this host does not offer blocking."
        }
        hint="Asks the helper which blocks it holds now."
        onClick={() => refresh.mutate()}
      >
        Refresh blocks
      </Button>
    </Stack>
  );
}

/**
 * What blocking looks like on one host: the leak self-test gate and the drills, the blocks
 * the hub recorded and how each stands, and Refresh. It is shown even where blocking is not
 * available, and says so; a failed load is an error with Retry, never an empty list.
 */
export function ContainmentHost({ host }: { host: string }) {
  const status = useHostContainment(host);
  return (
    <QueryBoundary
      query={status}
      context={`load the blocks on ${host}`}
      loading={<Skeleton shape="rows" />}
    >
      {(loaded) => <Details host={host} status={loaded} />}
    </QueryBoundary>
  );
}
