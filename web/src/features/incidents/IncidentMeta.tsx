import { Link } from "react-router";
import type { IncidentDetail } from "../../api/types";
import { RelativeTime, StatusPill, Text } from "../../components";
import { incidentStatusMeta, severityMeta, verdictMeta } from "../../design/tone";

/**
 * Everything under the title, once: the status and severity pills (and what triage
 * concluded, until the report card says it), then one muted line with where, which check,
 * since when and how often: `nas · disk.usage /srv · opened 3 hours ago · fired 14×`.
 */
export function IncidentMeta({ detail }: { detail: IncidentDetail }) {
  const { incident, host, report } = detail;
  const verdict = report ? undefined : incident.verdict;
  const check = incident.target ? `${incident.check} ${incident.target}` : incident.check;
  return (
    <>
      <StatusPill meta={incidentStatusMeta(incident.status)} />
      {verdict && <StatusPill meta={verdictMeta(verdict)} />}
      <StatusPill meta={severityMeta(incident.severity)} />
      <Text muted size="sm">
        <Link to={`/hosts/${encodeURIComponent(host)}`}>{host}</Link> · <Text mono>{check}</Text> ·
        opened <RelativeTime at={incident.opened_at} /> · fired {incident.occurrences}×
        {incident.status === "muted" && incident.muted_until && (
          <>
            {" "}
            · muted until <RelativeTime at={incident.muted_until} />
          </>
        )}
        {incident.resolved_at && (
          <>
            {" "}
            · resolved <RelativeTime at={incident.resolved_at} />
          </>
        )}
      </Text>
    </>
  );
}
