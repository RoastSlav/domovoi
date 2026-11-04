import styles from "./settings.module.css";
import { ApiFailure } from "../../api/types";

/**
 * A failed connection test as the UI shows it: the hub's top-level message in an alert,
 * with each returned issue on its own line naming the settings field it is about. Used by
 * both the settings section test button and the first-run wizard. Keys carry the issue
 * index so the same issue returned twice still renders.
 */
export function TestIssues({ error }: { error: unknown }) {
  if (!error) {
    return null;
  }
  const message = error instanceof ApiFailure ? error.message : "The test could not run.";
  const issues = error instanceof ApiFailure ? error.issues : [];
  return (
    <>
      <p role="alert" className={styles.alert}>
        {message}
      </p>
      {issues.map((issue, index) => (
        <p key={`${index}:${issue.path}:${issue.message}`} className={styles.status}>
          {issue.path ? `${issue.path}: ` : ""}
          {issue.message}
        </p>
      ))}
    </>
  );
}
