import type { InstallJob, InstallTarget } from "../../api/types";

export { isActive } from "../../api/hooks/packageJobs";

/** Target statuses from which a host may be tried again (with a fresh approval). */
const RETRYABLE = new Set(["failed", "not_run", "outcome_unknown"]);

/** Whether a target in `status` may be retried. */
export function canRetry(status: string): boolean {
  return RETRYABLE.has(status);
}

/** Whether a target in `status` may still be cancelled: only one that has not started. */
export function canCancel(status: string): boolean {
  return status === "queued";
}

/** Whether the person should look at the host: the result is unknown or was not confirmed. */
export function needsCheck(target: InstallTarget): boolean {
  return (
    target.status === "outcome_unknown" ||
    (target.status === "succeeded" && !target.outputs_confirmed)
  );
}

/** Every attempt of `hostId`, newest first. */
function attemptsOf(job: InstallJob, hostId: string): InstallTarget[] {
  return job.targets
    .filter((target) => target.host_id === hostId)
    .sort((left, right) => right.attempt - left.attempt);
}

/** The highest attempt of each host, in host name order. */
export function currentTargets(job: InstallJob): InstallTarget[] {
  const hosts = [...new Set(job.targets.map((target) => target.host_id))];
  return hosts
    .map((hostId) => attemptsOf(job, hostId)[0])
    .filter((target): target is InstallTarget => target !== undefined)
    .sort((left, right) => left.host_name.localeCompare(right.host_name));
}

/** The attempts of `hostId` before the current one, newest first. */
export function earlierAttempts(job: InstallJob, hostId: string): InstallTarget[] {
  return attemptsOf(job, hostId).slice(1);
}

/** The first eight characters of a plan hash, as the screens show it. */
export function planCode(hash: string): string {
  return hash.slice(0, 8);
}
