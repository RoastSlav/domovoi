import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { Button, Cluster, Section, Stack, StatusPill, Text } from "../../components";
import { stepStateMeta } from "../../design/tone";
import { EnrollDialog } from "../hosts/EnrollDialog";
import { ApplyStatus } from "./ApplyStatus";
import styles from "./setup.module.css";
import type { StepOutcome, Wizard } from "./useWizard";

function detail({ skipped, changed }: StepOutcome): string {
  if (skipped) {
    return "Skipped. Nothing was saved.";
  }
  if (changed === 0) {
    return "Nothing to change.";
  }
  return changed === 1 ? "1 setting saved." : `${changed} settings saved.`;
}

/** What each step did, with a link to where its settings can be changed later. */
function Summary({ outcomes }: { outcomes: StepOutcome[] }) {
  return (
    <ul className={styles.summary}>
      {outcomes.map((outcome) => (
        <li key={outcome.step.id} className={styles.summaryRow}>
          <Text strong>{outcome.step.title}</Text>
          <StatusPill meta={stepStateMeta(outcome.skipped ? "skipped" : "done")} />
          <Text muted>{detail(outcome)}</Text>
          <Link to={`/settings/${outcome.step.section}`}>
            Change {outcome.step.title} in Settings
          </Link>
        </li>
      ))}
    </ul>
  );
}

/**
 * The end of the first run: the settings are saved, so add the first host, then a summary of
 * what was set up and what was skipped. Finish leaves for the incidents page.
 */
export function FinishStep({ wizard }: { wizard: Wizard }) {
  const [adding, setAdding] = useState(false);
  const navigate = useNavigate();
  return (
    <Stack gap={5}>
      <Section
        title="Add your first host"
        description="Install the agent on a machine you want watched, then enroll it with a one-time command."
      >
        <Stack gap={4}>
          <ApplyStatus phase={wizard.apply.phase} />
          <Cluster>
            <Button icon="plus" onClick={() => setAdding(true)}>
              Add a host
            </Button>
            <Button
              variant="primary"
              onClick={() => void navigate("/incidents", { replace: true })}
            >
              Finish
            </Button>
          </Cluster>
        </Stack>
        {adding && <EnrollDialog onClose={() => setAdding(false)} />}
      </Section>
      <Section
        title="What you set up"
        description="Everything here can be changed later in Settings."
      >
        <Summary outcomes={wizard.outcomes()} />
      </Section>
    </Stack>
  );
}
