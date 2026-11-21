import { useMemo } from "react";
import { useHosts } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import {
  Button,
  DataTable,
  EmptyState,
  QueryBoundary,
  Section,
  Skeleton,
  StatusPill,
  type Column,
} from "../../components";
import { hostConnectionMeta, hostConnectionOf } from "../../design/tone";

/** The hosts to pick from when no host is selected: paths are confirmed host by host. */
export function HostChooser({ onChoose }: { onChoose: (name: string) => void }) {
  const hosts = useHosts();
  const columns = useMemo<Column<Host>[]>(
    () => [
      { key: "host", label: "Host", cell: (host) => host.name },
      {
        key: "connection",
        label: "Connection",
        cell: (host) => <StatusPill size="sm" meta={hostConnectionMeta(hostConnectionOf(host))} />,
      },
      {
        key: "show",
        label: "Show paths",
        hideLabel: true,
        cell: (host) => (
          <Button
            size="sm"
            aria-label={`Show paths of ${host.name}`}
            onClick={() => onChoose(host.name)}
          >
            Show paths
          </Button>
        ),
      },
    ],
    [onChoose],
  );
  return (
    <Section title="Choose a host" description="Management paths are confirmed host by host.">
      <QueryBoundary
        query={hosts}
        context="load hosts"
        loading={<Skeleton shape="rows" count={3} />}
        isEmpty={(data) => data.length === 0}
        empty={<EmptyState icon="server" title="No hosts yet" />}
      >
        {(data) => (
          <DataTable caption="Hosts" columns={columns} rows={data} rowKey={(host) => host.id} />
        )}
      </QueryBoundary>
    </Section>
  );
}
