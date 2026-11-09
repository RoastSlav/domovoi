import { useSearchParams } from "react-router";
import { useLogs, LOGS_REFRESH_MS } from "../../api/hooks/system";
import { ApiFailure } from "../../api/types";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/EmptyState";
import { PageIntro } from "../../components/PageIntro";
import styles from "./logs.module.css";

/** The sources the page can read; agent logs come later. */
const SOURCES = [{ value: "hub", text: "Hub" }];

/** Levels to ask for, most severe first; each one also asks for the levels above it. */
const LEVELS = [
  { value: "error", text: "Errors" },
  { value: "warn", text: "Warnings" },
  { value: "info", text: "Info" },
  { value: "debug", text: "Debug" },
];

/** How many lines to ask for; the hub's own default is 200. */
const COUNTS = [
  { value: "200", text: "200 lines" },
  { value: "100", text: "100 lines" },
  { value: "1000", text: "1000 lines" },
];

const DEFAULT_LEVEL = "info";
const DEFAULT_COUNT = "200";

/** What a failed log call says to a person. */
function failureText(error: unknown): string {
  if (error instanceof ApiFailure) {
    return error.message;
  }
  return "Something went wrong. Try again.";
}

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

/** One dropdown on the page; picking an option rewrites the URL, so the page asks again. */
function Choice({
  name,
  label,
  options,
  selected,
  onChange,
}: {
  name: string;
  label: string;
  options: { value: string; text: string }[];
  selected: string;
  onChange: (value: string) => void;
}) {
  const id = `choice-${name}`;
  return (
    <div className={styles.choice}>
      <label htmlFor={id}>{label}</label>
      <select id={id} value={selected} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.text}
          </option>
        ))}
      </select>
    </div>
  );
}

/**
 * The hub's own recent log lines, oldest first with the newest at the bottom. The page asks
 * again on its own every 5 seconds, and the Refresh button asks right away.
 */
export function LogsPage() {
  const [params, setParams] = useSearchParams();
  const level = params.get("level") ?? DEFAULT_LEVEL;
  const count = params.get("lines") ?? DEFAULT_COUNT;
  const source = params.get("source") ?? "hub";
  const captured = source === "hub";
  const logs = useLogs(level, Number(count), captured);
  const items = logs.data ?? [];

  const choose = (name: string, value: string) => {
    const next = new URLSearchParams(params);
    next.set(name, value);
    setParams(next);
  };

  return (
    <>
      <h1>Logs</h1>
      <PageIntro topic="logs" docs="concepts/logs.html">
        What the hub itself was doing. The newest lines are at the bottom, and secrets are removed
        before a line is shown.
      </PageIntro>
      <form className={styles.choices} aria-label="Choose the log lines to read">
        <Choice
          name="source"
          label="Source"
          options={SOURCES}
          selected={source}
          onChange={(value) => choose("source", value)}
        />
        <Choice
          name="level"
          label="Level and above"
          options={LEVELS}
          selected={level}
          onChange={(value) => choose("level", value)}
        />
        <Choice
          name="lines"
          label="Lines"
          options={COUNTS}
          selected={count}
          onChange={(value) => choose("lines", value)}
        />
      </form>
      {!captured && (
        <EmptyState title="Only hub logs">
          <p>Agent logs are not here yet.</p>
        </EmptyState>
      )}
      {captured && (
        <>
          {logs.isPending && <p>Loading…</p>}
          {logs.error && (
            <EmptyState title="Could not load logs">
              <p>{failureText(logs.error)}</p>
            </EmptyState>
          )}
          {logs.data && items.length === 0 && (
            <EmptyState title="No log lines">
              <p>Nothing was logged at {level} or above since the hub started.</p>
            </EmptyState>
          )}
          {items.length > 0 && (
            <pre className={styles.block} aria-label="Hub log lines" tabIndex={0}>
              {items.map((line, position) => (
                <span key={position} data-level={line.level} className={lineClass(line.level)}>
                  {`${clockTime(line.at)} ${line.level.toUpperCase()} ${line.target}: ${line.message}\n`}
                </span>
              ))}
            </pre>
          )}
          <div className={styles.refresh}>
            <span className={styles.hint}>Auto-refresh every {LOGS_REFRESH_MS / 1000} s</span>
            <Button onClick={() => void logs.refetch()}>Refresh</Button>
          </div>
        </>
      )}
    </>
  );
}
