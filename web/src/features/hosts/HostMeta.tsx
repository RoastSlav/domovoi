import type { ReactNode } from "react";
import { Link } from "react-router";
import type { Host } from "../../api/types";
import { Count, RelativeTime, StatusPill, Tag, Text, Tooltip } from "../../components";
import { hostConnectionMeta, hostConnectionOf, hostKindMeta } from "../../design/tone";
import { openIncidentsNoun } from "./hostFacts";
import { protocolDetail } from "./hostProtocol";
import styles from "./hostMeta.module.css";

/** Whether the host is the hub's own machine and no agent was ever enrolled on it.
 * The hub keeps its own host as `local`; enrolled agents survive restarts, so the
 * missing flag, not the lost presence, marks the hub-only state. */
export function isHubOnly(host: Host): boolean {
  return host.kind === "local" && !host.agent_enrolled;
}

/** One fact of the strip: a small label over its value. */
function Fact({ label, children }: { children: ReactNode; label: string }) {
  return (
    <div className={styles.fact}>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function Agent({ host }: { host: Host }) {
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
    return <Text muted>{host.agent_enrolled ? "Unknown" : "None"}</Text>;
  }
  return (
    <>
      <Text mono>{host.version}</Text>
      {host.outdated && (
        <Tooltip content="This agent is older than the hub. Update it to get the latest checks.">
          <span>
            <Text size="xs">
              <span className={styles.warn}>Update available</span>
            </Text>
          </span>
        </Tooltip>
      )}
    </>
  );
}

function Incidents({ host }: { host: Host }) {
  if (host.open_incidents === 0) {
    return <Text muted>None</Text>;
  }
  const noun = openIncidentsNoun(host.open_incidents);
  return (
    <Link to={`/incidents?host=${encodeURIComponent(host.name)}`} className={styles.incidents}>
      <Count tone="danger" value={host.open_incidents} label={noun} />
      <span aria-hidden="true">open</span>
    </Link>
  );
}

/** When the agent connected, or when it was last seen; nothing when neither is known. */
function Seen({ host }: { host: Host }) {
  if (host.connected && host.connected_since) {
    return (
      <Fact label="Connected since">
        <RelativeTime at={host.connected_since} />
      </Fact>
    );
  }
  if (!host.connected && host.last_seen) {
    return (
      <Fact label="Last seen">
        <RelativeTime at={host.last_seen} />
      </Fact>
    );
  }
  return null;
}

/**
 * What the page header says about a host, as a strip of facts: each a small label over its
 * value, divided by hairlines. Status, kind, the agent's version, open incidents (a link to
 * them) and when the agent connected or was last seen.
 */
export function HostMeta({ host }: { host: Host }) {
  const kind = hostKindMeta(host.kind);
  return (
    <dl className={styles.facts}>
      <Fact label="Status">
        <StatusPill meta={hostConnectionMeta(hostConnectionOf(host))} />
      </Fact>
      <Fact label="Kind">
        <Tooltip content={kind.hint}>
          <span>
            <Text>{kind.label}</Text>
          </span>
        </Tooltip>
      </Fact>
      <Fact label="Agent">
        <Agent host={host} />
      </Fact>
      <Fact label="Open incidents">
        <Incidents host={host} />
      </Fact>
      <Seen host={host} />
    </dl>
  );
}
