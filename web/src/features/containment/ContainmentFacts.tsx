import type { ContainmentStep } from "../../api/types";
import { InlineMessage, KeyValueList, Stack, Text, formatRemaining } from "../../components";
import {
  BRIDGE_GAP_TEXT,
  FLOOD_NOTE,
  UNVERIFIED_WARNING,
  existingFlowsText,
  unverifiedReason,
} from "../../design/containmentText";
import { UnprotectedList } from "./UnprotectedList";

/** The warning that coverage could not be verified, with its reason and the paths. */
export function UnverifiedNote({ step }: { step: ContainmentStep }) {
  return (
    <InlineMessage tone="warning" title="Coverage cannot be verified">
      <Stack gap={2}>
        <Text block>{UNVERIFIED_WARNING}</Text>
        <Text block muted size="sm">
          {unverifiedReason(step.reason)}
        </Text>
        <UnprotectedList paths={step.unprotected_paths} />
      </Stack>
    </InlineMessage>
  );
}

/** What a block is, in plain words: the source, how long, and what happens to open flows. */
export function ContainmentFacts({ step }: { step: ContainmentStep }) {
  const flows = existingFlowsText(step.existing_flows);
  return (
    <Stack gap={2}>
      <KeyValueList
        mono
        items={[
          { label: "Source", value: step.source },
          { label: "Blocked for", value: formatRemaining(step.ttl_s * 1000) },
          {
            label: "Approval",
            value: step.assurance === "verified" ? "Coverage verified" : "Coverage not verified",
          },
        ]}
      />
      {flows !== null && (
        <Text block size="sm">
          {flows}
        </Text>
      )}
      {step.bridge_gap === true && (
        <Text block size="sm">
          {BRIDGE_GAP_TEXT}
        </Text>
      )}
      <Text block muted size="sm">
        {FLOOD_NOTE}
      </Text>
    </Stack>
  );
}
