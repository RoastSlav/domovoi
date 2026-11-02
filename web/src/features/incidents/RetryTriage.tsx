import { useRetryTriage } from "../../api/hooks/incidents";
import { ApiFailure, type TriageState } from "../../api/types";
import { Button } from "../../components/Button";
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
  const retry = useRetryTriage(incidentId);
  // A report is already on the card; only a skip or a failure is worth a line of its own.
  const state =
    retry.data ?? (triage?.kind === "skipped" || triage?.kind === "failed" ? triage : undefined);
  return (
    <div className={styles.toolbar}>
      <Button onClick={() => retry.mutate()} disabled={retry.isPending}>
        {retry.isPending ? "Triaging…" : "Retry triage"}
      </Button>
      {state && (
        <p role="status" className={styles.triage}>
          {describe(state)}
        </p>
      )}
      {retry.error && (
        <p className={styles.error} role="alert">
          {retry.error instanceof ApiFailure ? retry.error.message : "Something went wrong."}
        </p>
      )}
    </div>
  );
}
