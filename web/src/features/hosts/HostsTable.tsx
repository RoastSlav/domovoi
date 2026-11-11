import { useMemo, useState } from "react";
import { Link } from "react-router";
import type { Host } from "../../api/types";
import {
  Cluster,
  DataTable,
  RelativeTime,
  StatusPill,
  Tag,
  Text,
  Tooltip,
  type Column,
} from "../../components";
import { hostConnectionMeta, hostConnectionOf, hostKindMeta } from "../../design/tone";
import { HostRowMenu, hostPath } from "./HostRowMenu";
import { DEFAULT_HOST_SORT, nextSort, sortHosts } from "./hostSort";

function Connection({ host }: { host: Host }) {
  const connection = hostConnectionOf(host);
  return (
    <Cluster gap={2}>
      <StatusPill meta={hostConnectionMeta(connection)} />
      {connection === "offline" && host.last_seen && (
        <Text muted size="xs">
          last seen <RelativeTime at={host.last_seen} />
        </Text>
      )}
    </Cluster>
  );
}

function Kind({ host }: { host: Host }) {
  return (
    <Tooltip content={hostKindMeta(host.kind).hint}>
      <span>
        <Tag>{hostKindMeta(host.kind).label}</Tag>
      </span>
    </Tooltip>
  );
}

function Version({ host }: { host: Host }) {
  if (host.refused) {
    return (
      <Tooltip content={hostConnectionMeta("refused").hint}>
        <span>
          <Tag tone="danger">{host.refused}</Tag>
        </span>
      </Tooltip>
    );
  }
  if (!host.version) {
    return <Text muted>–</Text>;
  }
  return (
    <Cluster gap={2}>
      <Text mono>{host.version}</Text>
      {host.outdated && (
        <Tooltip content="This agent is older than the hub. Update it to get the latest checks.">
          <span>
            <Tag tone="warning">Update available</Tag>
          </span>
        </Tooltip>
      )}
    </Cluster>
  );
}

function OpenIncidents({ host }: { host: Host }) {
  if (host.open_incidents === 0) {
    return <Text muted>0</Text>;
  }
  return (
    <Link to={`/incidents?host=${encodeURIComponent(host.name)}`}>
      <Tag tone="danger">{host.open_incidents}</Tag>
      <span className="visually-hidden"> open incidents on {host.name}</span>
    </Link>
  );
}

function LastSeen({ host }: { host: Host }) {
  if (host.connected) {
    return <Text muted>now</Text>;
  }
  return host.last_seen ? <RelativeTime at={host.last_seen} /> : <Text muted>–</Text>;
}

const COLUMNS: Column<Host>[] = [
  {
    key: "name",
    label: "Host",
    sortable: true,
    cell: (host) => (
      <Cluster gap={2}>
        <span>{host.name}</span>
      </Cluster>
    ),
  },
  {
    key: "connection",
    label: "Connection",
    sortable: true,
    hint: "Whether the host's agent is connected to the hub right now.",
    cell: (host) => <Connection host={host} />,
  },
  { key: "kind", label: "Kind", sortable: true, cell: (host) => <Kind host={host} /> },
  {
    key: "version",
    label: "Agent version",
    sortable: true,
    cell: (host) => <Version host={host} />,
  },
  {
    key: "incidents",
    label: "Open incidents",
    numeric: true,
    sortable: true,
    cell: (host) => <OpenIncidents host={host} />,
  },
  { key: "seen", label: "Last seen", sortable: true, cell: (host) => <LastSeen host={host} /> },
  {
    key: "actions",
    label: "Actions",
    hideLabel: true,
    cell: (host) => <HostRowMenu host={host} />,
  },
];

/**
 * The hosts as a sortable table, offline ones first until the person picks another order. A
 * host's name opens its page; the row's menu holds the rest of what can be done to it.
 */
export function HostsTable({ hosts }: { hosts: readonly Host[] }) {
  const [sort, setSort] = useState(DEFAULT_HOST_SORT);
  const rows = useMemo(() => sortHosts(hosts, sort), [hosts, sort]);
  return (
    <DataTable
      caption="Hosts"
      columns={COLUMNS}
      rows={rows}
      rowKey={(host) => host.id}
      rowHref={(host) => hostPath(host.name)}
      rowTone={(host) => {
        const tone = hostConnectionMeta(hostConnectionOf(host)).tone;
        return tone === "danger" ? tone : undefined;
      }}
      sort={sort}
      onSort={(key) => setSort((now) => nextSort(now, key))}
    />
  );
}
