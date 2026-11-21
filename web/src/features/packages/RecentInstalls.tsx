import { useInstallJobs } from "../../api/hooks/packageJobs";
import type { InstallJob } from "../../api/types";
import {
  DataTable,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  StatusPill,
  Text,
  visibilityAttrs,
  type Column,
} from "../../components";
import { installJobStatusMeta } from "../../design/tone";

/** The host names of a job, at most two, then "+N". */
function hostsText(job: InstallJob): string {
  const names = [...new Set(job.targets.map((target) => target.host_name))];
  const shown = names.slice(0, 2).join(", ");
  return names.length > 2 ? `${shown} +${names.length - 2}` : shown;
}

const COLUMNS: Column<InstallJob>[] = [
  {
    key: "package",
    label: "Package",
    cell: (job) => (
      <span {...visibilityAttrs("P0", "packages.jobs.row")}>
        {job.kind === "uninstall" ? "Remove " : ""}
        {job.name} <Text mono>{job.version}</Text>
      </span>
    ),
  },
  { key: "hosts", label: "Hosts", cell: hostsText },
  {
    key: "status",
    label: "Status",
    cell: (job) => <StatusPill meta={installJobStatusMeta(job.status)} />,
  },
  { key: "started", label: "Started", cell: (job) => <RelativeTime at={job.created_at} /> },
];

/** The ten newest install jobs; each row opens that job's page. */
export function RecentInstalls() {
  const jobs = useInstallJobs(10);
  return (
    <QueryBoundary
      query={jobs}
      context="load recent installs"
      loading={<Skeleton shape="rows" />}
      isEmpty={(page) => page.items.length === 0}
      empty={
        <EmptyState icon="package" title="No installs yet">
          <p>Installs you start appear here.</p>
        </EmptyState>
      }
    >
      {(page) => (
        <DataTable
          caption="Recent installs"
          columns={COLUMNS}
          rows={page.items}
          rowKey={(job) => job.id}
          rowHref={(job) => `/capabilities/jobs/${job.id}`}
        />
      )}
    </QueryBoundary>
  );
}
