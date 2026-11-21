import { useState } from "react";
import { useSearchParams } from "react-router";
import { useHosts } from "../../api/hooks/hosts";
import { usePackagesStatus } from "../../api/hooks/packages";
import type { Host } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  InlineMessage,
  LinkButton,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  Stepper,
  VisibilityPage,
  type StepperStep,
} from "../../components";
import { WizardChoose } from "./WizardChoose";
import { WizardHosts } from "./WizardHosts";
import { WizardReview } from "./WizardReview";
import { windowWords } from "./packageText";
import { initialState, type Step, type WizardState } from "./wizardState";

const ORDER: Step[] = ["choose", "hosts", "review"];
const LABELS = ["Package", "Hosts", "Review", "Confirm", "Progress"];

/** The stepper's steps: earlier ones done, the current one current, Confirm while the dialog is open. */
function stepsFor(step: Step, confirming: boolean): StepperStep[] {
  const at = ORDER.indexOf(step) + (confirming ? 1 : 0);
  return LABELS.map((label, index) => ({
    id: label.toLowerCase(),
    label,
    state: index === 4 ? "upcoming" : index < at ? "done" : index === at ? "current" : "upcoming",
  }));
}

/** Why the wizard cannot go on while installation is off or its state is unknown. */
function blockedReason(status: ReturnType<typeof usePackagesStatus>): string | undefined {
  if (status.data === undefined) {
    return status.error
      ? "Could not check whether package installation is on."
      : "Checking whether package installation is on.";
  }
  return status.data.enabled
    ? undefined
    : "Package installation is off on this hub. Turn it on in Settings, Packages.";
}

function Wizard({ hosts }: { hosts: Host[] }) {
  const [params] = useSearchParams();
  const [selected] = useSelectedHost();
  const status = usePackagesStatus();
  const [state, setState] = useState<WizardState>(() => initialState(params, hosts, selected));
  const [fixed] = useState(() => params.has("package") && params.has("version"));
  const [confirming, setConfirming] = useState(false);
  const set = (patch: Partial<WizardState>) => setState((now) => ({ ...now, ...patch }));
  const blocked = blockedReason(status);
  const minutes = windowWords(status.data?.approval_window_secs ?? 600);
  const go = (step: Step) => set({ step });

  return (
    <Stack gap={5}>
      <Stepper label="Install steps" steps={stepsFor(state.step, confirming)} />
      {status.data?.enabled === false && (
        <InlineMessage
          tone="info"
          title="Package installation is off"
          action={<LinkButton to="/settings/packages">Open Settings</LinkButton>}
        >
          Turn it on in Settings, Packages, then come back.
        </InlineMessage>
      )}
      {state.step === "choose" && (
        <WizardChoose
          state={state}
          set={set}
          fixed={fixed}
          blocked={blocked}
          onNext={() => go("hosts")}
        />
      )}
      {state.step === "hosts" && (
        <WizardHosts
          state={state}
          set={set}
          minutes={minutes}
          blocked={blocked}
          onBack={() => go("choose")}
          onNext={() => go("review")}
        />
      )}
      {state.step === "review" && (
        <WizardReview
          request={{
            kind: state.kind,
            name: state.name,
            version: state.version,
            hostIds: state.hostIds,
          }}
          minutes={minutes}
          blocked={blocked}
          confirming={confirming}
          setConfirming={setConfirming}
          onBack={() => go("hosts")}
        />
      )}
    </Stack>
  );
}

/**
 * Installs or removes a signed package on chosen hosts, one decision per step: package,
 * hosts, review of the plan, then confirmation with the person's password and the typed
 * package name. Opening it with parameters sends nothing; only the confirm button creates a job.
 */
export function InstallWizardPage() {
  const hosts = useHosts();
  return (
    <VisibilityPage page="capabilities">
      <PageHeader
        title="Install package"
        intro="Put an owner-signed package on your hosts, or take one off. Installing does not enable it."
        docs="guide/packages.html"
        actions={<LinkButton to="/capabilities?tab=packages">Back to packages</LinkButton>}
      />
      <QueryBoundary query={hosts} context="load hosts" loading={<Skeleton shape="rows" />}>
        {(list) => <Wizard hosts={list} />}
      </QueryBoundary>
    </VisibilityPage>
  );
}
