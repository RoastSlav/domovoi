import { useState, type FormEvent } from "react";
import { useHostChecks, useSaveHostChecks, type Overrides } from "../../api/hooks/settings";
import { ApiFailure, type HostCheck } from "../../api/types";
import { Button } from "../../components/Button";
import styles from "./hosts.module.css";

interface Row {
  enabled: "" | "on" | "off";
  interval: string;
  thresholds: string;
}

function rowOf(check: HostCheck): Row {
  const o = check.overrides;
  return {
    enabled: o?.enabled == null ? "" : o.enabled ? "on" : "off",
    interval: o?.interval_seconds == null ? "" : String(o.interval_seconds),
    thresholds:
      o?.thresholds && Object.keys(o.thresholds).length > 0 ? JSON.stringify(o.thresholds) : "",
  };
}

function local(check: HostCheck): string {
  const r = check.reported;
  if (!r) {
    return "not reported while the agent is offline";
  }
  const parts = [r.enabled ? "on" : "off", `every ${r.interval_seconds ?? "?"} s`];
  if (r.thresholds && Object.keys(r.thresholds).length > 0) {
    parts.push(JSON.stringify(r.thresholds));
  }
  return parts.join(", ");
}

function overridesOf(rows: Record<string, Row>): Overrides | string {
  const out: Overrides = {};
  for (const [key, row] of Object.entries(rows)) {
    let thresholds: Record<string, unknown> = {};
    if (row.thresholds.trim()) {
      try {
        thresholds = JSON.parse(row.thresholds) as Record<string, unknown>;
      } catch {
        return `${key}: thresholds are not valid JSON.`;
      }
    }
    const interval = row.interval.trim() ? Number(row.interval) : null;
    if (interval !== null && (!Number.isInteger(interval) || interval < 1)) {
      return `${key}: the interval is a whole number of seconds.`;
    }
    if (row.enabled || interval !== null || Object.keys(thresholds).length > 0) {
      out[key] = {
        enabled: row.enabled === "" ? null : row.enabled === "on",
        interval_seconds: interval,
        thresholds,
      };
    }
  }
  return out;
}

/**
 * A host's checks: what its agent.toml sets, and the hub's overrides on top. Saving sends
 * them to the agent at once, or at its next connection when it is offline.
 */
export function ChecksTab({ host }: { host: string }) {
  const checks = useHostChecks(host);
  const save = useSaveHostChecks(host);
  const [rows, setRows] = useState<Record<string, Row>>();
  const [problem, setProblem] = useState<string>();
  const [result, setResult] = useState<string>();

  if (checks.isPending) {
    return <p>Loading…</p>;
  }
  if (checks.error) {
    return <p>Could not load the checks.</p>;
  }
  const current = rows ?? Object.fromEntries(checks.data.checks.map((c) => [c.key, rowOf(c)]));
  const update = (key: string, change: Partial<Row>) =>
    setRows({ ...current, [key]: { ...(current[key] as Row), ...change } });

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const overrides = overridesOf(current);
    if (typeof overrides === "string") {
      setProblem(overrides);
      return;
    }
    setProblem(undefined);
    setResult(undefined);
    save.mutate(overrides, {
      onSuccess: (saved) =>
        setResult(saved.delivered ? "Sent to agent." : "Agent offline, will apply on reconnect."),
      onError: (error) =>
        setProblem(error instanceof ApiFailure ? error.message : "Could not save the checks."),
    });
  };

  if (checks.data.checks.length === 0) {
    return (
      <p>
        {checks.data.connected
          ? "The agent reported no checks."
          : "The agent is offline, so its checks are not known yet."}
      </p>
    );
  }
  return (
    <form className={styles.form} onSubmit={onSubmit} noValidate>
      {!checks.data.connected && (
        <p>The agent is offline. Changes are saved and sent when it reconnects.</p>
      )}
      <table className={styles.checks}>
        <thead>
          <tr>
            <th scope="col">Check</th>
            <th scope="col">From agent.toml</th>
            <th scope="col">Runs</th>
            <th scope="col">Interval (s)</th>
            <th scope="col">Thresholds (JSON)</th>
          </tr>
        </thead>
        <tbody>
          {checks.data.checks.map((check) => {
            const row = current[check.key] as Row;
            return (
              <tr key={check.key}>
                <th scope="row">
                  <code>{check.key}</code>
                </th>
                <td>{local(check)}</td>
                <td>
                  <select
                    aria-label={`${check.key} runs`}
                    value={row.enabled}
                    onChange={(e) =>
                      update(check.key, { enabled: e.target.value as Row["enabled"] })
                    }
                  >
                    <option value="">as agent.toml</option>
                    <option value="on">on</option>
                    <option value="off">off</option>
                  </select>
                </td>
                <td>
                  <input
                    aria-label={`${check.key} interval in seconds`}
                    inputMode="numeric"
                    value={row.interval}
                    placeholder="as agent.toml"
                    onChange={(e) => update(check.key, { interval: e.target.value })}
                  />
                </td>
                <td>
                  <input
                    aria-label={`${check.key} thresholds`}
                    value={row.thresholds}
                    spellCheck={false}
                    placeholder="{}"
                    onChange={(e) => update(check.key, { thresholds: e.target.value })}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {problem && (
        <p className={styles.error} role="alert">
          {problem}
        </p>
      )}
      {result && <p role="status">{result}</p>}
      <div>
        <Button type="submit" variant="primary" disabled={save.isPending}>
          Save checks
        </Button>
      </div>
    </form>
  );
}
