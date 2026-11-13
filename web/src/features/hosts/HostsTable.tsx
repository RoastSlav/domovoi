import { useMemo, useState } from "react";
import { Link } from "react-router";
import type { Host } from "../../api/types";
import {
  Cluster,
  Count,
  DataTable,
  RelativeTime,
  StatusPill,
  Tag,
  Text,
  Tooltip,
  type Column,
} from "../../components";
import { hostConnectionMeta, hostConnectionOf, hostKindMeta } from "../../design/tone";
import { openIncidentsNoun } from "./hostFacts";
import { protocolDetail } from "./hostProtocol";
import { HostRowMenu, hostPath } from "./HostRowMenu";
import { DEFAULT_HOST_SORT, nextSort, sortHosts } from "./hostSort";

function Connection({ host }: { host: Host }) {
  return <StatusPill meta={hostConnectionMeta(hostConnectionOf(host))} />;
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
  const detail = protocolDetail(host);
  if (detail) {
    const connection = hostConnectionOf(host);
    return (
      <Tooltip content={hostConnectionMeta(connection).hint}>
        <span>
          <Tag tone={connection === "incompatible" ? "danger" : "warning"}>{detail}</Tag>
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

/** A count chip in both cases, so zeros and counts share one right edge; zero is the quiet one. */
function OpenIncidents({ host }: { host: Host }) {
  if (host.open_incidents === 0) {
    return <Count tone="neutral" value={0} label={openIncidentsNoun(0)} />;
  }
  return (
    <Link
      to={`/incidents?host=${encodeURIComponent(host.name)}`}
      aria-label={`${host.open_incidents} ${openIncidentsNoun(host.open_incidents)} on ${host.name}`}
    >
      <Count
        tone="danger"
        value={host.open_incidents}
        label={openIncidentsNoun(host.open_incidents)}
      />
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
    truncate: "clamp2",
    tip: (host) => host.name,
    cell: (host) => host.name,
  },
  {
    key: "connection",
    label: "Connection",
    width: "11rem",
    sortable: true,
    hint: "Whether the host's agent is connected to the hub right now.",
    cell: (host) => <Connection host={host} />,
  },
  {
    key: "kind",
    label: "Kind",
    width: "8rem",
    sortable: true,
    cell: (host) => <Kind host={host} />,
  },
  {
    key: "version",
    label: "Agent version",
    width: "12rem",
    sortable: true,
    cell: (host) => <Version host={host} />,
  },
  {
    key: "incidents",
    label: "Open incidents",
    width: "9rem",
    numeric: true,
    sortable: true,
    cell: (host) => <OpenIncidents host={host} />,
  },
  {
    key: "seen",
    label: "Last seen",
    width: "9rem",
    sortable: true,
    cell: (host) => <LastSeen host={host} />,
  },
  {
    key: "actions",
    label: "Actions",
    width: "4rem",
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
      layout="fixed"
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
