import styles from "./Badge.module.css";

const LABELS: Record<string, string> = {
  ACTION_NEEDED: "Action needed",
  MONITOR: "Monitor",
  EXPECTED: "Expected",
  FIXED: "Fixed",
  critical: "Critical",
  warning: "Warning",
  info: "Info",
};

const TONES: Record<string, string> = {
  ACTION_NEEDED: "action-needed",
  MONITOR: "monitor",
  EXPECTED: "expected",
  FIXED: "fixed",
  critical: "critical",
  warning: "warning",
  info: "info",
};

/** The text a badge shows for a verdict or severity name. */
export function badgeLabel(value: string): string {
  return LABELS[value] ?? value;
}

/**
 * A verdict or severity as the chat cards show it: a coloured label that always carries
 * its text, so colour is never the only signal.
 */
export function Badge({ value }: { value: string }) {
  return (
    <span className={styles.badge} data-tone={TONES[value] ?? "expected"}>
      {badgeLabel(value)}
    </span>
  );
}
