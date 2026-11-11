import { Link } from "react-router";
import { useActions } from "../../api/hooks/actions";
import { useHosts } from "../../api/hooks/hosts";
import type { Action } from "../../api/types";
import {
  DataTable,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Section,
  Skeleton,
  StatusPill,
  type Column,
} from "../../components";
import { actionStatusMeta } from "../../design/tone";

/** How many runs the list shows. */
const MAX_RUNS = 10;

const COLUMNS: Column<Action>[] = [
  {
    key: "status",
    label: "Status",
    cell: (run) => <StatusPill size="sm" meta={actionStatusMeta(run.status)} />,
  },
  { key: "when", label: "Updated", cell: (run) => <RelativeTime at={run.updated_at} /> },
  {
    key: "incident",
    label: "Incident",
    cell: (run) => (
      <Link to={`/incidents/${encodeURIComponent(run.incident_id)}`}>{run.incident_id}</Link>
    ),
  },
];

/** The latest actions that ran `skill` on `host`, newest first, each linking to its incident. */
export function SkillRuns({ host, skill }: { host: string; skill: string }) {
  const actions = useActions();
  const hosts = useHosts();
  const hostId = (hosts.data ?? []).find((h) => h.name === host)?.id;
  return (
    <Section title={`Recent runs on ${host}`}>
      <QueryBoundary
        query={actions}
        context="load recent runs"
        loading={<Skeleton shape="rows" count={2} />}
        isEmpty={(page) => !page.items.some((a) => a.host_id === hostId && a.skill === skill)}
        empty={
          <EmptyState icon="tool" title="No runs yet">
            <p>{`${skill} has not run on ${host}. Runs appear here once triage proposes it and an action is decided.`}</p>
          </EmptyState>
        }
      >
        {(page) => (
          <DataTable
            caption={`Recent runs of ${skill} on ${host}`}
            columns={COLUMNS}
            rows={page.items
              .filter((a) => a.host_id === hostId && a.skill === skill)
              .slice(0, MAX_RUNS)}
            rowKey={(run) => run.id}
          />
        )}
      </QueryBoundary>
    </Section>
  );
}
