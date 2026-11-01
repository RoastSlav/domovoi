import { useState } from "react";
import { useAck, useMute, useUnmute } from "../../api/hooks/incidents";
import { ApiFailure, type IncidentSummary } from "../../api/types";
import { Button } from "../../components/Button";
import styles from "./incidents.module.css";

const MUTE_OPTIONS: [number, string][] = [
  [60, "1 hour"],
  [240, "4 hours"],
  [1440, "1 day"],
  [10080, "1 week"],
];

/** Ack, Mute and Unmute, as far as the incident's status allows. */
export function IncidentControls({ incident }: { incident: IncidentSummary }) {
  const [minutes, setMinutes] = useState(MUTE_OPTIONS[0]?.[0] ?? 60);
  const ack = useAck(incident.id);
  const mute = useMute(incident.id);
  const unmute = useUnmute(incident.id);
  const error = ack.error ?? mute.error ?? unmute.error;

  if (incident.status === "resolved") {
    return null;
  }
  return (
    <div className={styles.toolbar}>
      {incident.status !== "acked" && (
        <Button onClick={() => ack.mutate()} disabled={ack.isPending}>
          Ack
        </Button>
      )}
      {incident.status === "muted" ? (
        <Button onClick={() => unmute.mutate()} disabled={unmute.isPending}>
          Unmute
        </Button>
      ) : (
        <>
          <div className={styles.filter}>
            <label htmlFor="mute-for">Mute for</label>
            <select
              id="mute-for"
              value={minutes}
              onChange={(e) => setMinutes(Number(e.target.value))}
            >
              {MUTE_OPTIONS.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </div>
          <Button onClick={() => mute.mutate(minutes)} disabled={mute.isPending}>
            Mute
          </Button>
        </>
      )}
      {error && (
        <p className={styles.error} role="alert">
          {error instanceof ApiFailure ? error.message : "Something went wrong."}
        </p>
      )}
    </div>
  );
}
