import { Link } from "react-router";
import { useIncidents } from "../../api/hooks/incidents";
import type { IncidentSummary } from "../../api/types";
import {
  DataTable,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  StatusPill,
  Text,
  type Column,
} from "../../components";
import { incidentStateMeta } from "../../design/tone";

/** How many incidents the overview lists; the rest are one link away. */
const RECENT = 10;

const COLUMNS: Column<IncidentSummary>[] = [
  {
    key: "title",
    label: "Incident",
    cell: (incident) => <Text strong>{incident.title}</Text>,
  },
  {
    key: "state",
    label: "Status",
    cell: (incident) => <StatusPill size="sm" meta={incidentStateMeta(incident)} />,
  },
  {
    key: "fired",
    label: "Last fired",
    cell: (incident) => <RelativeTime at={incident.last_firing_at} />,
  },
];

/** The host's latest incidents as a compact table; a failed request is an error with Retry. */
export function RecentIncidents({ host }: { host: string }) {
  const incidents = useIncidents({ host });
  const recent = incidents.data?.pages[0]?.items.slice(0, RECENT);
  return (
    <QueryBoundary
      query={{
        data: recent,
        error: incidents.error,
        isPending: incidents.isPending,
        refetch: incidents.refetch,
      }}
      context="load incidents"
      loading={<Skeleton shape="rows" count={3} />}
      isEmpty={(items) => items.length === 0}
      empty={
        <EmptyState icon="check-circle" title={`No incidents on ${host}`}>
          <p>Nothing has gone wrong here yet. When a check fails, its incident shows up here.</p>
        </EmptyState>
      }
    >
      {(items) => (
        <DataTable
          caption={`Recent incidents on ${host}`}
          columns={COLUMNS}
          rows={items}
          rowKey={(incident) => incident.id}
          rowHref={(incident) => `/incidents/${incident.id}`}
        />
      )}
    </QueryBoundary>
  );
}

/** The link to every incident on `host`, for the section's actions. */
export function AllIncidentsLink({ host }: { host: string }) {
  return (
    <Link to={`/incidents?host=${encodeURIComponent(host)}`}>
      All incidents on <span>{host}</span>
    </Link>
  );
}
