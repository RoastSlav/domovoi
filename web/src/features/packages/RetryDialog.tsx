import { useState } from "react";
import { usePlanJob, useRetryTarget } from "../../api/hooks/packageJobs";
import type { InstallJob, InstallPlanTarget, InstallTarget } from "../../api/types";
import { describeTargetFailure } from "../../design/messages";
import { capabilityStateMeta } from "../../design/tone";
import {
  Button,
  ConfirmDialog,
  Dialog,
  QueryBoundary,
  Skeleton,
  Stack,
  StatusPill,
  Text,
  useActionFeedback,
} from "../../components";
import { PasswordField } from "./PasswordField";
import { PlanCode } from "./PlanCode";
import { useLockout } from "./useLockout";

/** The form half: only mounted once the single-host plan is in, so its hash is captured once. */
function RetryForm({
  job,
  target,
  plan,
  onClose,
}: {
  job: InstallJob;
  onClose: () => void;
  plan: InstallPlanTarget;
  target: InstallTarget;
}) {
  const [captured] = useState(plan.plan_hash);
  const [password, setPassword] = useState("");
  const [typed, setTyped] = useState("");
  const lockout = useLockout();
  const mutation = useRetryTarget(job.id);
  const retry = useActionFeedback(mutation, {
    success: `Started again on ${target.host_name}`,
    error: "start it again",
    errorDisplay: "inline",
  });
  const availability = capabilityStateMeta(plan.availability);
  const failure = describeTargetFailure(target.failure ?? { code: target.status, message: "" });
  const reason =
    plan.plan_hash !== captured
      ? "The plan changed while you were looking. Close this and review it again."
      : password === ""
        ? "Enter your password first."
        : lockout.reason;

  const confirm = () => {
    retry.mutate(
      { hostId: target.host_id, planHash: captured, password, confirmText: typed },
      {
        onSuccess: () => {
          mutation.reset();
          setPassword("");
          onClose();
        },
        onError: (error) => {
          mutation.reset();
          setPassword("");
          lockout.lock(error);
        },
      },
    );
  };

  return (
    <ConfirmDialog
      title={`Try ${job.name} ${job.version} again on ${target.host_name}?`}
      consequence={
        <Stack gap={2}>
          <span>
            Last time on {target.host_name}: {failure.title}
          </span>
          <span>
            <StatusPill meta={availability} /> {plan.availability_text}
          </span>
          <span>
            A retry is a new approval, bound to plan <PlanCode hash={captured} />.
          </span>
        </Stack>
      }
      confirmLabel="Retry"
      requireText={job.name}
      onTyped={setTyped}
      confirmDisabledReason={reason}
      busy={retry.isPending}
      error={retry.error}
      onConfirm={confirm}
      onCancel={onClose}
    >
      <PasswordField value={password} onChange={setPassword} />
    </ConfirmDialog>
  );
}

/**
 * Tries one host of a job again. It plans that host alone, shows the result, then asks for
 * a fresh approval (the password and the typed package name) bound to that host's plan hash.
 *
 * Not for starting a new install (that is the wizard) and never opened by anything but a
 * row's Retry button.
 */
export function RetryDialog({
  job,
  target,
  onClose,
}: {
  job: InstallJob;
  onClose: () => void;
  target: InstallTarget;
}) {
  const plan = usePlanJob({
    kind: job.kind === "uninstall" ? "uninstall" : "install",
    name: job.name,
    version: job.version,
    hostIds: [target.host_id],
  });
  const single = plan.data?.targets.find((item) => item.host_id === target.host_id);
  if (single) {
    return <RetryForm job={job} target={target} plan={single} onClose={onClose} />;
  }
  return (
    <Dialog
      title={`Try ${job.name} again on ${target.host_name}?`}
      onClose={onClose}
      actions={<Button onClick={onClose}>Close</Button>}
    >
      <QueryBoundary query={plan} context="work out the plan" loading={<Skeleton shape="rows" />}>
        {() => <Text>The hub did not plan {target.host_name}.</Text>}
      </QueryBoundary>
    </Dialog>
  );
}
