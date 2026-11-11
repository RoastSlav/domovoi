import { Link } from "react-router";
import type { IncidentSummary } from "../../api/types";
import { RelativeTime, StatusPill } from "../../components";
import { incidentBadgeMeta } from "../../design/tone";
import { triageNote } from "./badge";
import styles from "./incidents.module.css";

/** One incident in a list: its badge, title, host, status, when it last fired and triage's note. */
export function IncidentRow({
  incident,
  host,
}: {
  incident: IncidentSummary;
  host: string | undefined;
}) {
  const note = triageNote(incident);
  return (
    <li className={styles.row}>
      <div className={styles.rowHead}>
        <StatusPill meta={incidentBadgeMeta(incident)} />
        <Link to={`/incidents/${incident.id}`} className={styles.title}>
          {incident.title}
        </Link>
      </div>
      <p className={styles.meta}>
        {host ?? "unknown host"} · {incident.status} · last fired{" "}
        <RelativeTime at={incident.last_firing_at} />
        {incident.occurrences > 1 && ` · ${incident.occurrences} times`}
        {note && ` · ${note}`}
      </p>
    </li>
  );
}
