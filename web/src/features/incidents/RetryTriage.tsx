import { useRetryTriage } from "../../api/hooks/incidents";
import type { TriageState } from "../../api/types";
import { Button, ErrorDetail, InlineMessage, useActionFeedback } from "../../components";
import styles from "./incidents.module.css";

/** One line on how a triage run went. */
function describe(state: TriageState): string {
  switch (state.kind) {
    case "report":
      return "Triage ran.";
    case "cached":
      return "Triage reused an earlier report for the same evidence.";
    case "skipped":
      return `Triage was skipped: ${state.reason ?? "no reason given"}.`;
    case "failed":
      return `Triage failed: ${state.reason ?? "no reason given"}.`;
    default:
      return `Unknown triage state ${state.kind}.`;
  }
}

/** Runs triage again for the incident and shows how the attempt went. */
export function RetryTriage({
  incidentId,
  triage,
}: {
  incidentId: string;
  triage: TriageState | null | undefined;
}) {
  const mutation = useRetryTriage(incidentId);
  const retry = useActionFeedback(mutation, {
    success: (state) =>
      state.kind === "report" || state.kind === "cached" ? "Triage finished" : null, // The line under the button says why there is no report.
    error: "run triage",
    errorDisplay: "inline",
  });
  // A report is already on the card; only a skip or a failure is worth a line of its own.
  const state =
    mutation.data ?? (triage?.kind === "skipped" || triage?.kind === "failed" ? triage : undefined);
  const fresh = triage?.kind === "report" || triage?.kind === "cached";
  return (
    <div className={styles.toolbar}>
      <Button onClick={() => retry.mutate(fresh)} loading={retry.isPending}>
        {fresh ? "Run triage again" : "Retry triage"}
      </Button>
      <p className={styles.triage}>
        Runs the model again with the current skills, notes and evidence. Counts against the hourly
        triage limit.
      </p>
      {state && (
        <p role="status" className={styles.triage}>
          {describe(state)}
        </p>
      )}
      {retry.error && (
        <InlineMessage tone="danger" title={retry.error.title}>
          <ErrorDetail error={retry.error} />
        </InlineMessage>
      )}
    </div>
  );
}
