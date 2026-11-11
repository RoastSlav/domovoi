import type { LogLine } from "../../api/types";
import { Button, EmptyState, LoadError, Loading } from "../../components";
import { LOGS_REFRESH_MS } from "../../api/hooks/system";
import styles from "./logs.module.css";

/** The level and line count asked for when a reader has not picked. */
export const DEFAULT_LEVEL = "info";
export const DEFAULT_LINES = 200;

/** The colour class a line is drawn in for its level; unrecognised levels stay plain. */
function lineClass(level: string) {
  if (level === "error") {
    return styles.error;
  }
  if (level === "warn") {
    return styles.warn;
  }
  if (level === "debug") {
    return styles.debug;
  }
  return styles.line;
}

/** The time of day a line was logged, as `HH:MM:SS`, in the browser's own zone. */
function clockTime(at: string): string {
  const date = new Date(at);
  const two = (value: number) => String(value).padStart(2, "0");
  return `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
}

/**
 * Log lines oldest first, newest at the bottom, with what to say while loading, when the
 * call failed or when nothing was logged, and a Refresh button. Shared by the Logs page
 * and the Logs tab of the host page, so both read the same way.
 */
export function LogViewer({
  logs,
  level,
  label,
}: {
  logs: {
    items: LogLine[];
    error: unknown;
    loading: boolean;
    refresh: () => unknown;
  };
  level: string;
  label: string;
}) {
  return (
    <>
      {logs.loading && <Loading />}
      {logs.error != null && (
        <LoadError error={logs.error} context="load logs" onRetry={logs.refresh} />
      )}
      {logs.error == null && !logs.loading && logs.items.length === 0 && (
        <EmptyState title="No log lines">
          <p>Nothing was logged at {level} or above.</p>
        </EmptyState>
      )}
      {logs.items.length > 0 && (
        <pre className={styles.block} aria-label={label} tabIndex={0}>
          {logs.items.map((line, position) => (
            <span key={position} data-level={line.level} className={lineClass(line.level)}>
              {`${clockTime(line.at)} ${line.level.toUpperCase()} ${line.target}: ${line.message}\n`}
            </span>
          ))}
        </pre>
      )}
      <div className={styles.refresh}>
        <span className={styles.hint}>Auto-refresh every {LOGS_REFRESH_MS / 1000} s</span>
        <Button icon="refresh" onClick={() => void logs.refresh()}>
          Refresh
        </Button>
      </div>
    </>
  );
}
