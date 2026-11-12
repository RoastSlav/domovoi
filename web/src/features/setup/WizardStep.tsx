import { ApiFailure, type Editable } from "../../api/types";
import { Button, Cluster, InlineError, InlineMessage, Section, Stack } from "../../components";
import { DiscordSetupHelp } from "../settings/DiscordSetupHelp";
import { TestIssues } from "../settings/TestIssues";
import { ApplyStatus } from "./ApplyStatus";
import { StepFields } from "./StepFields";
import { STEPS, type Step } from "./steps";
import type { Wizard } from "./useWizard";

/** The result of the last connection test, in words, or why it could not run. */
function TestResult({ wizard }: { wizard: Wizard }) {
  const { data, error } = wizard.tester;
  return (
    <>
      {data && (
        <InlineMessage
          tone={data.ok ? "success" : "danger"}
          title={data.ok ? "The test worked" : "The test failed"}
        >
          {data.message}
        </InlineMessage>
      )}
      <TestIssues error={error} />
    </>
  );
}

/**
 * One step of the first run as a section: its fields, a Test button where a connection can
 * be tried, Skip with what skipping leaves out, and Back and Next. The last step saves.
 */
export function WizardStep({
  step,
  editable,
  wizard,
}: {
  editable: Editable;
  step: Step;
  wizard: Wizard;
}) {
  const last = wizard.step === STEPS.length - 1;
  const issues = wizard.saveError instanceof ApiFailure ? wizard.saveError.issues : [];
  return (
    <Section title={step.title} description={step.intro}>
      <Stack gap={4}>
        {step.id === "discord" && <DiscordSetupHelp />}
        <StepFields fields={step.fields} editable={editable} wizard={wizard} issues={issues} />
        <Cluster gap={2} justify="between">
          <Cluster gap={2}>
            {wizard.step > 0 && (
              <Button icon="chevron-left" onClick={wizard.back}>
                Back
              </Button>
            )}
            {step.test && (
              <Button
                loading={wizard.tester.isPending}
                hint="Tries these values without saving them."
                onClick={wizard.testStep}
              >
                Test
              </Button>
            )}
            {step.optional && (
              <Button
                variant="ghost"
                icon="skip"
                hint={step.skipHint}
                disabledReason={wizard.apply.pending ? "Saving the settings." : undefined}
                onClick={() => wizard.next(true)}
              >
                Skip
              </Button>
            )}
          </Cluster>
          <Button
            variant="primary"
            loading={wizard.apply.pending}
            onClick={() => wizard.next(false)}
          >
            {last ? "Save and apply" : "Next"}
          </Button>
        </Cluster>
        <TestResult wizard={wizard} />
        {wizard.saveError !== undefined && (
          <InlineError error={wizard.saveError} context="save the settings" />
        )}
        <ApplyStatus phase={wizard.apply.phase} />
      </Stack>
    </Section>
  );
}
