import styles from "./Badge.module.css";

const LABELS: Record<string, string> = {
  ACTION_NEEDED: "Action needed",
  MONITOR: "Monitor",
  EXPECTED: "Expected",
  FIXED: "Fixed",
  resolved: "Resolved",
  critical: "Critical",
  warning: "Warning",
  info: "Info",
};

const TONES: Record<string, string> = {
  ACTION_NEEDED: "action-needed",
  MONITOR: "monitor",
  EXPECTED: "expected",
  FIXED: "fixed",
  resolved: "fixed",
  critical: "critical",
  warning: "warning",
  info: "info",
};

const HINTS: Record<string, string> = {
  ACTION_NEEDED: "Something is broken or will break soon; act now.",
  MONITOR: "Off but not urgent; keep an eye on it.",
  EXPECTED: "Explained by something intended, like a planned update or a nightly job.",
  FIXED: "Already recovered.",
  resolved: "The problem cleared.",
};

/** The text a badge shows for a verdict, severity or resolved state name. */
export function badgeLabel(value: string): string {
  return LABELS[value] ?? value;
}

/** What a badge's value means, as its tooltip; undefined for values nothing explains. */
export function badgeHint(value: string): string | undefined {
  return HINTS[value];
}

/**
 * A verdict, severity or resolved state as the cards show it: a coloured label that always
 * carries its text, so colour is never the only signal, and says what it means on hover.
 */
export function Badge({ value }: { value: string }) {
  return (
    <span className={styles.badge} data-tone={TONES[value] ?? "expected"} title={badgeHint(value)}>
      {badgeLabel(value)}
    </span>
  );
}
