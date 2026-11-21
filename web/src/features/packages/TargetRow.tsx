import { useState } from "react";
import { Link } from "react-router";
import { useCancelTarget } from "../../api/hooks/packageJobs";
import { useRefreshHostPackages } from "../../api/hooks/hostPackages";
import type { InstallJob, InstallTarget } from "../../api/types";
import { describeTargetFailure } from "../../design/messages";
import { installTargetStatusMeta, privilegeBoundaryMeta, type Tone } from "../../design/tone";
import {
  Button,
  Countdown,
  ErrorDetail,
  InlineMessage,
  Progress,
  RelativeTime,
  Stack,
  StatusPill,
  Text,
  Visible,
  useActionFeedback,
  useRemaining,
  visibilityAttrs,
} from "../../components";
import { canCancel, canRetry, needsCheck } from "./jobModel";
import { RetryDialog } from "./RetryDialog";
import styles from "./wizard.module.css";

/** What a failed, never-run or unknown target says, in the owner's words. */
function failureText(target: InstallTarget): { detail?: string; title: string } {
  if (target.failure) {
    return describeTargetFailure(target.failure);
  }
  if (target.status === "not_run") {
    return describeTargetFailure({ code: "not_run", message: "" });
  }
  return { title: installTargetStatusMeta(target.status).hint };
}

/** How long a queued host has left before the hub drops the install. */
function Waiting({ target }: { target: InstallTarget }) {
  const remaining = useRemaining(target.expires_at);
  return (
    <Text block>
      {`Waiting for ${target.host_name}. It is dropped in `}
      <Countdown remaining={remaining} until={target.expires_at} />
      {" if the host is not ready."}
    </Text>
  );
}

/** The words under a target's status pill. */
function Result({ target }: { target: InstallTarget }) {
  const meta = installTargetStatusMeta(target.status);
  switch (target.status) {
    case "queued":
      return <Waiting target={target} />;
    case "transferring":
      return <Progress label={`Sending to ${target.host_name}…`} />;
    case "dispatched":
      return <Progress label={`Running on ${target.host_name}. Waiting for the result…`} />;
    case "failed":
    case "not_run":
    case "outcome_unknown": {
      const text = failureText(target);
      return (
        <InlineMessage tone={meta.tone as Tone} title={text.title}>
          {text.detail}
        </InlineMessage>
      );
    }
    case "succeeded":
      return target.outputs_confirmed ? null : (
        <Text block>
          Confirmed by the host&apos;s record only. Check the host to see what is installed.
        </Text>
      );
    default:
      return null;
  }
}

/** One host of a job: its current attempt, what to do about it, and its earlier attempts. */
export function TargetRow({
  job,
  target,
  earlier,
}: {
  earlier: InstallTarget[];
  job: InstallJob;
  target: InstallTarget;
}) {
  const [retrying, setRetrying] = useState(false);
  const [checked, setChecked] = useState<string>();
  const cancel = useActionFeedback(useCancelTarget(job.id), {
    success: `Cancelled the install on ${target.host_name}`,
    error: "cancel the install",
    errorDisplay: "inline",
  });
  const refresh = useRefreshHostPackages(target.host_id);
  const check = useActionFeedback(refresh, {
    success: () => null,
    error: "check the host",
    errorDisplay: "inline",
  });
  const meta = installTargetStatusMeta(target.status);
  const subject = `${job.name} ${job.version}`;
  const failed = cancel.error ?? check.error;
  const acts = canRetry(target.status) || canCancel(target.status) || needsCheck(target);

  return (
    <li className={styles.target} {...visibilityAttrs("P1", "job.target.row")}>
      <div className={styles.targetHead}>
        <Link to={`/hosts/${encodeURIComponent(target.host_name)}`}>{target.host_name}</Link>
        <Text muted size="sm">
          <RelativeTime at={target.finished_at ?? target.started_at ?? target.queued_at} />
        </Text>
      </div>
      <div {...visibilityAttrs("P0", "job.target.status")}>
        <Stack gap={2}>
          <div className={styles.hostState}>
            <StatusPill meta={meta} />
            {target.privilege_boundary && (
              <StatusPill size="sm" meta={privilegeBoundaryMeta(target.privilege_boundary)} />
            )}
          </div>
          <Result target={target} />
          {checked && <InlineMessage tone="info">{checked}</InlineMessage>}
          {failed && (
            <InlineMessage tone="danger" title={failed.title}>
              <ErrorDetail error={failed} />
            </InlineMessage>
          )}
        </Stack>
      </div>
      {acts && (
        <div {...visibilityAttrs("P0", "job.target.actions")}>
          <div className={styles.hostState}>
            {canRetry(target.status) && <Button onClick={() => setRetrying(true)}>Retry</Button>}
            {canCancel(target.status) && (
              <Button
                hint="Only an install that has not started can be cancelled."
                loading={cancel.isPending}
                onClick={() => cancel.mutate({ hostId: target.host_id })}
              >
                Cancel
              </Button>
            )}
            {needsCheck(target) && (
              <Button
                icon="refresh"
                loading={check.isPending}
                onClick={() =>
                  check.mutate(undefined, {
                    onSuccess: (answer) => {
                      const found = answer.entries.some(
                        (entry) => entry.name === job.name && entry.version === job.version,
                      );
                      setChecked(
                        found
                          ? `${target.host_name} reports ${subject} installed`
                          : `${target.host_name} does not list ${subject}`,
                      );
                    },
                  })
                }
              >
                Check host
              </Button>
            )}
          </div>
        </div>
      )}
      {earlier.length > 0 && (
        <Visible
          category="P2"
          id="job.target.history"
          label="Earlier attempts"
          count={earlier.length}
        >
          <ul className={styles.earlier} aria-label={`Earlier attempts on ${target.host_name}`}>
            {earlier.map((attempt) => (
              <li key={attempt.id}>
                {`Attempt ${attempt.attempt}: `}
                <StatusPill size="sm" meta={installTargetStatusMeta(attempt.status)} />{" "}
                {attempt.failure ? describeTargetFailure(attempt.failure).title : null}
              </li>
            ))}
          </ul>
        </Visible>
      )}
      {retrying && <RetryDialog job={job} target={target} onClose={() => setRetrying(false)} />}
    </li>
  );
}
