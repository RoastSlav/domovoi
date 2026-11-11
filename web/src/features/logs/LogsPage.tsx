import { useSearchParams } from "react-router";
import { useHosts, useHostLogs } from "../../api/hooks/hosts";
import { LOGS_REFRESH_MS, useLogs } from "../../api/hooks/system";
import { useSelectedHost } from "../../app/HostContext";
import {
  Field,
  OptionPicker,
  PageHeader,
  Select,
  StatusPill,
  type PickerOption,
} from "../../components";
import { hostConnectionMeta, hostConnectionOf } from "../../design/tone";
import { LogViewer } from "./LogViewer";
import { DEFAULT_LEVEL, DEFAULT_LINES } from "./logLines";
import styles from "./logs.module.css";

/** The value of the source that is the hub's own log. */
const HUB = "hub";

/** Levels to ask for, most severe first; each one also asks for the levels above it. */
const LEVELS = [
  { value: "error", text: "Errors" },
  { value: "warn", text: "Warnings" },
  { value: "info", text: "Info" },
  { value: "debug", text: "Debug" },
];

/** How many lines to ask for; the hub's own default is 200. */
const COUNTS = [
  { value: "100", text: "100 lines" },
  { value: "200", text: "200 lines" },
  { value: "1000", text: "1000 lines" },
];

/**
 * Recent log lines, oldest first with the newest at the bottom: the hub's own, or a host
 * agent's. Until a source is picked here, it is the host selected for the whole app, or the
 * hub under "All hosts". Level, line count, source and the Live switch live in the URL, so a
 * view can be shared and survives a reload.
 */
export function LogsPage() {
  const [params, setParams] = useSearchParams();
  const level = params.get("level") ?? DEFAULT_LEVEL;
  const count = params.get("lines") ?? String(DEFAULT_LINES);
  const [selected] = useSelectedHost();
  const source = params.get("source") ?? selected ?? HUB;
  const captured = source === HUB;
  const live = params.get("refresh") !== "off";
  const hosts = useHosts();
  const hub = useLogs(level, Number(count), captured, live);
  const agent = useHostLogs(captured ? "" : source, level, Number(count), !captured, live);

  const known = hosts.data ?? [];
  const sources: PickerOption[] = [
    { value: HUB, label: "Hub" },
    // A source the hub does not list (a stale link) still shows, so it can be seen and changed.
    ...(!captured && !known.some((host) => host.name === source)
      ? [{ value: source, label: source }]
      : []),
    ...known.map((host) => ({
      value: host.name,
      label: host.name,
      meta: <StatusPill size="sm" meta={hostConnectionMeta(hostConnectionOf(host))} />,
    })),
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
      <form
        className={styles.choices}
        aria-label="Choose the log lines to read"
        onSubmit={(event) => event.preventDefault()}
      >
        <div className={styles.pickerField}>
          <span className={styles.pickerLabel}>Source</span>
          <OptionPicker
            label="Source"
            value={source}
            options={sources}
            onChange={(value) => choose("source", value)}
          />
        </div>
        <Field label="Level and above">
          <Select value={level} onChange={(event) => choose("level", event.target.value)}>
            {LEVELS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.text}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Lines">
          <Select value={count} onChange={(event) => choose("lines", event.target.value)}>
            {COUNTS.map((option) => (
              <option key={option.value} value={option.value}>
                {option.text}
              </option>
            ))}
          </Select>
        </Field>
      </form>
      <LogViewer
        key={source}
        query={captured ? hub : agent}
        level={level}
        label={captured ? "Hub log lines" : `Agent log lines on ${source}`}
        refreshMs={live ? LOGS_REFRESH_MS : null}
        onLiveChange={(next) => choose("refresh", next ? "on" : "off")}
      />
    </>
  );
}
