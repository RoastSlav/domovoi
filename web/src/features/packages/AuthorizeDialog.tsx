import { useState } from "react";
import { useNavigate } from "react-router";
import { useCreateInstallJob, type PlanRequest } from "../../api/hooks/packageJobs";
import { ApiFailure, type InstallPlan } from "../../api/types";
import { ConfirmDialog, useActionFeedback } from "../../components";
import { PasswordField } from "./PasswordField";
import { PlanCode } from "./PlanCode";
import { useLockout } from "./useLockout";

const CHANGED = "The plan changed while you were looking. Close this and review it again.";

/**
 * The one place an install or removal is authorized: the person's password and the package
 * name typed by them, bound to the plan hash captured when this dialog opened. If the plan
 * changes under it, confirming is blocked; a `plan_changed` answer closes it for a new review.
 *
 * Not for retrying one host (that is `RetryDialog`) and never opened by anything but the
 * review step's button.
 */
export function AuthorizeDialog({
  plan,
  request,
  onClose,
  onPlanChanged,
}: {
  onClose: () => void;
  onPlanChanged: () => void;
  plan: InstallPlan;
  request: PlanRequest;
}) {
  const navigate = useNavigate();
  const [captured] = useState(plan.job_plan_hash);
  const [password, setPassword] = useState("");
  const [typed, setTyped] = useState("");
  const lockout = useLockout();
  const removing = request.kind === "uninstall";
  const count = request.hostIds.length;
  const mutation = useCreateInstallJob();
  const create = useActionFeedback(mutation, {
    success: () =>
      `Started ${removing ? "removing" : "installing"} ${request.name} on ${count} host${count === 1 ? "" : "s"}`,
    error: removing ? "start the removal" : "start the install",
    errorDisplay: "inline",
  });
  const hosts = plan.targets.map((target) => target.host_name).join(", ");

  const reason =
    plan.job_plan_hash !== captured
      ? CHANGED
      : password === ""
        ? "Enter your password first."
        : lockout.reason;

  const confirm = () => {
    const sent = password;
    create.mutate(
      { request, planHash: captured, password: sent, confirmText: typed },
      {
        onSuccess: (job) => {
          mutation.reset();
          setPassword("");
          void navigate(`/capabilities/jobs/${job.id}`);
        },
        onError: (error) => {
          mutation.reset();
          setPassword("");
          lockout.lock(error);
          if (error instanceof ApiFailure && error.code === "plan_changed") {
            onPlanChanged();
          }
        },
      },
    );
  };

  return (
    <ConfirmDialog
      title={`${removing ? "Remove" : "Install"} ${request.name} ${request.version} on ${count} host${count === 1 ? "" : "s"}?`}
      consequence={
        <>
          {removing
            ? `${request.name} ${request.version} is removed from ${hosts}. `
            : `${request.name} ${request.version} is sent to ${hosts} and installed there. It is not enabled. `}
          This approval is bound to plan <PlanCode hash={captured} />.
        </>
      }
      confirmLabel={removing ? "Remove" : "Install"}
      tone={removing ? "danger" : "primary"}
      requireText={request.name}
      onTyped={setTyped}
      confirmDisabledReason={reason}
      busy={create.isPending}
      error={create.error}
      onConfirm={confirm}
      onCancel={onClose}
    >
      <PasswordField value={password} onChange={setPassword} />
    </ConfirmDialog>
  );
}
