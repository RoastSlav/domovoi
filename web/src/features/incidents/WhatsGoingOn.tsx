import { Link } from "react-router";
import { useRetryTriage } from "../../api/hooks/incidents";
import type { IncidentDetail } from "../../api/types";
import {
  Button,
  Card,
  Cluster,
  ErrorDetail,
  InlineMessage,
  Progress,
  Stack,
  StatusPill,
  Text,
  useActionFeedback,
} from "../../components";
import { sentence } from "../../design/messages";
import { triageMeta, verdictMeta } from "../../design/tone";
import { MODEL_SETTINGS, type ModelOff } from "./useModelOff";
import styles from "./incidents.module.css";

const RETRY_HINT =
  "Runs the model again with the current skills, notes and evidence. Counts against the hourly triage limit.";

/** What triage said, as far as the incident has it: from the report, or from the chat card. */
function lead(detail: IncidentDetail) {
  const { report, card, incident } = detail;
  if (report) {
    return {
      verdict: report.verdict,
      headline: report.headline,
      summary: report.summary,
      cause: report.cause,
      next: report.next_step,
    };
  }
  const done = detail.triage?.kind === "report" || detail.triage?.kind === "cached";
  if (!done) {
    return undefined;
  }
  return {
    verdict: incident.verdict ?? undefined,
    // Before a report the card's headline is the title again.
    headline: card.headline.trim() !== incident.title.trim() ? card.headline : "",
    summary: "",
    cause: "",
    next: card.next_step ?? "",
  };
}

/**
 * The top of the incident page: what is going on and why, in triage's own words (the
 * headline, the summary, the likely cause, and the next step set apart). Without a report
 * the same place says how triage went, compactly: running, skipped, failed, or not set up.
 * "Run triage again" is a small button in the corner.
 */
export function WhatsGoingOn({
  detail,
  off,
}: {
  detail: IncidentDetail;
  off: ModelOff | undefined;
}) {
  const incidentId = detail.incident.id;
  const mutation = useRetryTriage(incidentId);
  const retry = useActionFeedback(mutation, {
    // A skip or a failure is said here, where the reason has room.
    success: (state) =>
      state.kind === "report" || state.kind === "cached" ? "Triage finished" : null,
    error: "run triage",
    errorDisplay: "inline",
  });
  const state = mutation.data ?? detail.triage ?? undefined;
  const said = lead(detail);
  const hasReport = state?.kind === "report" || state?.kind === "cached";
  const label = hasReport ? "Run triage again" : state ? "Retry triage" : "Run triage";
  const again = (variant: "ghost" | "secondary") => (
    <Button
      size="sm"
      variant={variant}
      icon="refresh"
      hint={RETRY_HINT}
      loading={retry.isPending}
      disabledReason={off?.reason}
      // With a report in hand, only a fresh run can say anything new.
      onClick={() =>
        retry.mutate(detail.triage?.kind === "report" || detail.triage?.kind === "cached")
      }
    >
      {label}
    </Button>
  );
  const failure = retry.error && (
    <InlineMessage tone="danger" title={retry.error.title}>
      <ErrorDetail error={retry.error} />
    </InlineMessage>
  );

  if (retry.isPending && !said) {
    return (
      <Card label="What's going on">
        <Progress label="Triage is reading the logs…" />
      </Card>
    );
  }
  if (!said && (state?.kind === "skipped" || state?.kind === "failed")) {
    return (
      <Stack gap={2}>
        <InlineMessage
          tone={triageMeta(state.kind).tone}
          title={state.kind === "skipped" ? "Triage was skipped" : "Triage failed"}
          action={again("secondary")}
        >
          {state.reason ? sentence(state.reason) : "The hub gave no reason."}
        </InlineMessage>
        {failure}
      </Stack>
    );
  }
  if (!said) {
    return (
      <Card label="What's going on">
        <Cluster justify="between">
          <Text muted>
            {off ? (
              <>
                Triage has not run, and it cannot: {off.line.replace(/\.$/, "").toLowerCase()}.{" "}
                <Link to={MODEL_SETTINGS}>Open model settings</Link>
              </>
            ) : (
              "Triage has not run for this incident."
            )}
          </Text>
          {again("secondary")}
        </Cluster>
        {failure}
      </Card>
    );
  }

  const verdict = said.verdict ? verdictMeta(said.verdict) : undefined;
  return (
    <Card tone={verdict?.tone} label="What's going on">
      <div className={styles.going}>
        <div className={styles.goingHead}>
          <Cluster>
            {verdict && <StatusPill meta={verdict} />}
            {said.headline && (
              <Text strong size="lg">
                {said.headline}
              </Text>
            )}
          </Cluster>
        </div>
        <div className={styles.goingBody}>
          <Stack gap={3}>
            {retry.isPending && <Progress label="Triage is reading the logs…" />}
            {said.summary && <Text block>{said.summary}</Text>}
            {said.cause && (
              <Text block>
                <Text muted>Cause: </Text>
                {said.cause}
              </Text>
            )}
            {said.next && (
              <div className={styles.nextStep}>
                <strong>Next step:</strong> {said.next}
              </div>
            )}
            {state?.kind === "cached" && (
              <Text muted size="sm" block>
                Triage reused the report of an earlier incident with the same evidence.
              </Text>
            )}
            {failure}
          </Stack>
        </div>
        <div className={styles.goingAgain}>{again("ghost")}</div>
      </div>
    </Card>
  );
}
