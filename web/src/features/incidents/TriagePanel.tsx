import { useRetryTriage } from "../../api/hooks/incidents";
import type { TriageState } from "../../api/types";
import {
  Button,
  Cluster,
  ErrorDetail,
  InlineMessage,
  Progress,
  Section,
  Stack,
  Text,
  useActionFeedback,
} from "../../components";
import { sentence } from "../../design/messages";
import { triageMeta } from "../../design/tone";

const RETRY_HINT =
  "Runs the model again with the current skills, notes and evidence. Counts against the hourly triage limit.";

/**
 * How triage went for the incident, and running it again: progress while it runs, the
 * reason in words when it was skipped or failed, a quiet note when a report was reused.
 */
export function TriagePanel({
  incidentId,
  triage,
}: {
  incidentId: string;
  triage: TriageState | null | undefined;
}) {
  const mutation = useRetryTriage(incidentId);
  const retry = useActionFeedback(mutation, {
    // A skip or a failure is said in the panel, where the reason has room.
    success: (state) =>
      state.kind === "report" || state.kind === "cached" ? "Triage finished" : null,
    error: "run triage",
    errorDisplay: "inline",
  });
  const state = mutation.data ?? triage ?? undefined;
  const hasReport = state?.kind === "report" || state?.kind === "cached";
  const again = (
    <Button
      size="sm"
      hint={RETRY_HINT}
      loading={retry.isPending}
      // With a report in hand, only a fresh run can say anything new.
      onClick={() => retry.mutate(triage?.kind === "report" || triage?.kind === "cached")}
    >
      {hasReport ? "Run triage again" : state ? "Retry triage" : "Run triage"}
    </Button>
  );

  let body;
  if (retry.isPending) {
    body = <Progress label="Triage is reading the logs…" />;
  } else if (state?.kind === "skipped" || state?.kind === "failed") {
    body = (
      <InlineMessage
        tone={triageMeta(state.kind).tone}
        title={state.kind === "skipped" ? "Triage was skipped" : "Triage failed"}
        action={again}
      >
        {state.reason ? sentence(state.reason) : "The hub gave no reason."}
      </InlineMessage>
    );
  } else {
    body = (
      <Cluster justify="between">
        <Text muted size="sm">
          {state?.kind === "cached"
            ? "Triage reused the report of an earlier incident with the same evidence."
            : state?.kind === "report"
              ? "Triage ran and wrote the report above."
              : state
                ? triageMeta(state.kind).hint
                : "Triage has not run for this incident."}
        </Text>
        {again}
      </Cluster>
    );
  }

  return (
    <Section title="Triage">
      <Stack gap={2}>
        {body}
        {retry.error && (
          <InlineMessage tone="danger" title={retry.error.title}>
            <ErrorDetail error={retry.error} />
          </InlineMessage>
        )}
      </Stack>
    </Section>
  );
}
