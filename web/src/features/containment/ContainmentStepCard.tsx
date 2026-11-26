import { useState } from "react";
import type { ContainmentStep } from "../../api/types";
import {
  Button,
  Cluster,
  Section,
  Stack,
  StatusPill,
  Stepper,
  Text,
  Visible,
} from "../../components";
import { phaseNote } from "../../design/containmentText";
import { blockPhaseMeta } from "../../design/tone";
import { ContainmentFacts, UnverifiedNote } from "./ContainmentFacts";
import { phaseSteps } from "./phases";
import { ReleaseDialog } from "./ReleaseDialog";

/** The end of a confirmed block as the person reads it. */
function endText(until: string | null | undefined): string | null {
  if (!until) {
    return null;
  }
  const date = new Date(until);
  return Number.isNaN(date.getTime()) ? until : date.toLocaleString();
}

/**
 * A containment step on the step inspector: where the block stands (apply, confirm, then
 * blocked until, ended, removed or lost), what it does to flows that were already open, and
 * Release while it stands. The current state is always shown; the detail follows the profile.
 */
export function ContainmentStepCard({
  operationId,
  step,
  stepId,
}: {
  operationId: string;
  step: ContainmentStep;
  stepId: string;
}) {
  const [releasing, setReleasing] = useState(false);
  return (
    <Section title="Block">
      <Stack gap={3}>
        <Visible category="P0" id="containment.phase">
          <Stack gap={2}>
            <Cluster>
              <StatusPill meta={blockPhaseMeta(step.phase)} />
            </Cluster>
            {phaseNote(step.phase) !== null && (
              <Text block size="sm">
                {phaseNote(step.phase)}
              </Text>
            )}
            <Stepper
              label="Phases of the block"
              steps={phaseSteps(step.phase, endText(step.expires_at))}
            />
          </Stack>
        </Visible>
        {step.assurance === "unverified" && (
          <Visible category="P0" id="containment.unverified">
            <UnverifiedNote step={step} />
          </Visible>
        )}
        <Visible category="P1" id="containment.facts">
          <ContainmentFacts step={step} />
        </Visible>
        {step.phase === "confirmed" && (
          <Button
            variant="secondary"
            hint="Takes the block away now. Asks for your password and the source."
            onClick={() => setReleasing(true)}
          >
            Release block
          </Button>
        )}
      </Stack>
      {releasing && (
        <ReleaseDialog
          operationId={operationId}
          step={stepId}
          source={step.source}
          onClose={() => setReleasing(false)}
        />
      )}
    </Section>
  );
}
