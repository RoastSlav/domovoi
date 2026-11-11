import type { Editable } from "../../api/types";
import { useSettings } from "../../api/hooks/system";
import {
  EmptyState,
  LinkButton,
  LoadError,
  Loading,
  PageHeader,
  PasswordPrompt,
  Stack,
  Stepper,
  type StepperStep,
} from "../../components";
import { FinishStep } from "./FinishStep";
import { HOST_TITLE, STEPS } from "./steps";
import { useWizard } from "./useWizard";
import { WizardStep } from "./WizardStep";

function Wizard({ editable }: { editable: Editable }) {
  const wizard = useWizard(editable);
  const current = STEPS[wizard.step];
  const steps: StepperStep[] = [
    ...STEPS.map((s) => ({ id: s.id, label: s.title })),
    { id: "host", label: HOST_TITLE },
  ].map((s, i) => ({
    ...s,
    state:
      i === wizard.step
        ? ("current" as const)
        : i > wizard.step
          ? ("upcoming" as const)
          : wizard.skipped.has(i)
            ? ("skipped" as const)
            : ("done" as const),
  }));
  return (
    <Stack gap={4}>
      <Stepper label="Setup steps" steps={steps} />
      {current ? (
        <WizardStep step={current} editable={editable} wizard={wizard} />
      ) : (
        <FinishStep wizard={wizard} />
      )}
      {wizard.asking && (
        <PasswordPrompt
          reason="Saving a token or key needs your password again."
          error={wizard.passwordError}
          busy={wizard.apply.pending}
          onConfirm={wizard.confirmPassword}
          onCancel={wizard.cancelPassword}
        />
      )}
    </Stack>
  );
}

/** What the page shows once settings are known: a load failure, a read-only hub, or the steps. */
function Body() {
  const settings = useSettings();
  if (settings.isPending) {
    return <Loading />;
  }
  // A failed request is not "managed in hub.toml": the hub may simply not be reachable.
  if (settings.error) {
    return <LoadError error={settings.error} context="load settings" onRetry={settings.refetch} />;
  }
  // The hub answered without editable settings: it runs from a fixed hub.toml.
  if (!settings.data.editable) {
    return (
      <EmptyState
        title="Settings are managed in hub.toml"
        action={
          <LinkButton to="/incidents" variant="primary">
            Go to incidents
          </LinkButton>
        }
      >
        <p>
          This hub runs from hub.toml, so settings cannot be changed here. Edit hub.toml and restart
          the hub.
        </p>
      </EmptyState>
    );
  }
  return <Wizard editable={settings.data.editable} />;
}

/**
 * The guided first run after the admin account exists: hub basics, Discord, a model, the
 * digest and quiet hours, applied together, then the first host and a summary. Every step
 * but the first can be skipped; Settings has all of it later.
 */
export function FirstRun() {
  return (
    <>
      <PageHeader
        title="Set up Domovoi"
        intro="A few steps to get alerts, triage and your first host working. You can skip any step but the first and come back in Settings."
      />
      <Body />
    </>
  );
}
