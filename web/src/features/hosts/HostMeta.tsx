import { Link } from "react-router";
import type { Host } from "../../api/types";
import { RelativeTime, StatusPill, Tag, Text, Tooltip } from "../../components";
import { hostConnectionMeta, hostConnectionOf, hostKindMeta } from "../../design/tone";

/** Whether the host is the hub's own machine and no agent was ever enrolled on it.
 * The hub keeps its own host as `local`; enrolled agents survive restarts, so the
 * missing flag, not the lost presence, marks the hub-only state. */
export function isHubOnly(host: Host): boolean {
  return host.kind === "local" && !host.agent_enrolled;
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
    return null;
  }
  return (
    <>
      <Text mono muted>{`Agent ${host.version}`}</Text>
      {host.outdated && (
        <Tooltip content="This agent is older than the hub. Update it to get the latest checks.">
          <span>
            <Tag tone="warning">Update available</Tag>
          </span>
        </Tooltip>
      )}
    </>
  );
}

function Seen({ host }: { host: Host }) {
  if (host.connected && host.connected_since) {
    return (
      <Text muted>
        Connected since <RelativeTime at={host.connected_since} />
      </Text>
    );
  }
  if (!host.connected && host.last_seen) {
    return (
      <Text muted>
        Last seen <RelativeTime at={host.last_seen} />
      </Text>
    );
  }
  return null;
}

/**
 * What the page header says about a host, on one line where it fits: whether its agent is
 * connected, its kind, the agent's version, how many incidents are open (a link to them)
 * and when it was last seen.
 */
export function HostMeta({ host }: { host: Host }) {
  const kind = hostKindMeta(host.kind);
  return (
    <>
      <StatusPill meta={hostConnectionMeta(hostConnectionOf(host))} />
      <Tooltip content={kind.hint}>
        <span>
          <Tag>{kind.label}</Tag>
        </span>
      </Tooltip>
      <Version host={host} />
      {host.open_incidents > 0 && (
        <Link to={`/incidents?host=${encodeURIComponent(host.name)}`}>
          <Tag tone="danger">{host.open_incidents}</Tag>
          <span> open incidents</span>
        </Link>
      )}
      <Seen host={host} />
    </>
  );
}
