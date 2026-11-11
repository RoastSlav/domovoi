import { useSearchParams } from "react-router";
import { useHosts, useHostLogs } from "../../api/hooks/hosts";
import { useLogs } from "../../api/hooks/system";
import { PageHeader } from "../../components";
import { LogViewer, DEFAULT_LEVEL, DEFAULT_LINES } from "./LogViewer";
import styles from "./logs.module.css";

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

/** Whether the page asks on its own every 5 seconds, or only when the person clicks. */
const REFRESH = [
  { value: "on", text: "Every 5 seconds" },
  { value: "off", text: "Only when I click" },
];

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
 * Recent log lines, oldest first with the newest at the bottom: the hub's own, or a host
 * agent's. The page asks again on its own every 5 seconds while the Refresh dropdown is
 * on, and the Refresh button asks right away either way.
 */
export function LogsPage() {
  const [params, setParams] = useSearchParams();
  const level = params.get("level") ?? DEFAULT_LEVEL;
  const count = params.get("lines") ?? String(DEFAULT_LINES);
  const source = params.get("source") ?? "hub";
  const captured = source === "hub";
  const refresh = params.get("refresh") ?? "on";
  const auto = refresh !== "off";
  const hosts = useHosts();
  const hub = useLogs(level, Number(count), captured, auto);
  const agent = useHostLogs(captured ? "" : source, level, Number(count), !captured, auto);
  const sources = [
    { value: "hub", text: "Hub" },
    ...(hosts.data ?? []).map((host) => ({ value: host.name, text: host.name })),
  ];

  const choose = (name: string, value: string) => {
    const next = new URLSearchParams(params);
    next.set(name, value);
    setParams(next);
  };

  return (
    <>
      <PageHeader
        title="Logs"
        docs="concepts/logs.html"
        intro={
          captured ? (
            <>
              What the hub itself was doing. The newest lines are at the bottom, and secrets are
              removed before a line is shown.
            </>
          ) : (
            <>
              What the agent on <strong>{source}</strong> was doing. The newest lines are at the
              bottom, and secrets are removed before a line is shown, again here in case the agent
              was not careful enough.
            </>
          )
        }
      />
      <form className={styles.choices} aria-label="Choose the log lines to read">
        <Choice
          name="source"
          label="Source"
          options={sources}
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
        <Choice
          name="refresh"
          label="Auto-refresh"
          options={REFRESH}
          selected={refresh}
          onChange={(value) => choose("refresh", value)}
        />
      </form>
      {captured ? (
        <LogViewer
          logs={{
            items: hub.data ?? [],
            error: hub.error,
            loading: hub.isPending,
            refresh: () => hub.refetch(),
          }}
          level={level}
          label="Hub log lines"
        />
      ) : (
        <LogViewer
          logs={{
            items: agent.data ?? [],
            error: agent.error,
            loading: agent.isPending,
            refresh: () => agent.refetch(),
          }}
          level={level}
          label={`Agent log lines on ${source}`}
        />
      )}
    </>
  );
}
