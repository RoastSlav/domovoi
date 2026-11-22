import { Link } from "react-router";
import { useHostNames } from "../../../api/hooks/hosts";
import { Highlight, RelativeTime, StatusPill } from "../../../components";
import { severityMeta } from "../../../design/tone";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Incidents the hub found for the text, newest first, with a link to the full list. */
export function IncidentsGroup({ q, state }: { q: string; state: SearchGroups["incidents"] }) {
  const names = useHostNames();
  return (
    <ResultGroup
      group={SEARCH_GROUPS[0]}
      state={state}
      actions={<Link to={`/incidents?q=${encodeURIComponent(q)}`}>Open in Incidents</Link>}
      rows={state.items.map((incident) => ({
        key: incident.id,
        node: (
          <>
            <span className={styles.line}>
              <StatusPill meta={severityMeta(incident.severity)} size="sm" />
              <Link to={`/incidents/${encodeURIComponent(incident.id)}`}>
                <Highlight text={incident.title} query={q} />
              </Link>
            </span>
            <span className={styles.muted}>
              {names.get(incident.host_id) ?? incident.host_id} ·{" "}
              <RelativeTime at={incident.last_firing_at} />
            </span>
          </>
        ),
      }))}
    />
  );
}
