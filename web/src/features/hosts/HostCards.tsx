import { useMemo, useState } from "react";
import type { Host } from "../../api/types";
import {
  Button,
  Card,
  Cluster,
  Count,
  RelativeTime,
  StatusPill,
  Tag,
  Text,
  Tooltip,
} from "../../components";
import { hostConnectionMeta, hostConnectionOf, hostKindMeta } from "../../design/tone";
import { shortSpan } from "./duration";
import { EnrollDialog } from "./EnrollDialog";
import { openIncidentsNoun } from "./hostFacts";
import { HostRowMenu, hostPath } from "./HostRowMenu";
import { DEFAULT_HOST_SORT, sortHosts } from "./hostSort";
import styles from "./hostsList.module.css";

/**
 * One muted line saying how long the agent has been connected, or when it was last seen. A
 * host with no agent has nothing to say here: its pill already says so, and the card offers
 * to enroll one instead.
 */
function Seen({ host }: { host: Host }) {
  if (host.connected && host.connected_since) {
    const since = new Date(host.connected_since);
    return (
      <Text muted size="sm" block>
        Connected for{" "}
        <Tooltip content={since.toLocaleString()}>
          <time dateTime={host.connected_since}>{shortSpan(since, new Date())}</time>
        </Tooltip>
      </Text>
    );
  }
  if (host.connected) {
    return (
      <Text muted size="sm" block>
        Connected now
      </Text>
    );
  }
  if (host.last_seen) {
    return (
      <Text muted size="sm" block>
        Last seen <RelativeTime at={host.last_seen} />
      </Text>
    );
  }
  return hasNoAgent(host) ? null : (
    <Text muted size="sm" block>
      Not seen yet
    </Text>
  );
}

/** Whether the host has never had an agent, so the card can offer to enroll one; an external host reports by webhook and never gets one. */
function hasNoAgent(host: Host): boolean {
  if (host.kind === "external") {
    return false;
  }
  const connection = hostConnectionOf(host);
  return connection === "never_connected" || connection === "hub_only";
}

/** "Enroll agent" for a host without one, over the card's corner so it is not inside the link. */
function EnrollAction({ host }: { host: Host }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={styles.cardAction}>
      <Button size="sm" icon="plus" onClick={() => setOpen(true)}>
        Enroll agent
      </Button>
      {open && <EnrollDialog initialName={host.name} onClose={() => setOpen(false)} />}
    </div>
  );
}

function Incidents({ host }: { host: Host }) {
  if (host.open_incidents === 0) {
    return (
      <Text muted size="sm" block>
        No open incidents
      </Text>
    );
  }
  return (
    <Cluster gap={2}>
      <Count
        tone="danger"
        value={host.open_incidents}
        label={openIncidentsNoun(host.open_incidents)}
      />
      <span aria-hidden="true">
        <Text size="sm">{openIncidentsNoun(host.open_incidents)}</Text>
      </span>
    </Cluster>
  );
}

/** The agent's version in mono, or why it was refused, with a note when it is out of date. */
function Version({ host }: { host: Host }) {
  if (host.refused) {
    return <Tag tone="danger">Refused</Tag>;
  }
  if (!host.version) {
    return null;
  }
  return (
    <>
      <Text mono muted size="xs">
        {host.version}
      </Text>
      {host.outdated && <Tag tone="warning">Update available</Tag>}
    </>
  );
}

function HostCard({ host }: { host: Host }) {
  const connection = hostConnectionOf(host);
  const meta = hostConnectionMeta(connection);
  return (
    <div className={styles.cardWrap}>
      <Card
        to={hostPath(host.name)}
        label={host.name}
        tone={meta.tone === "danger" ? "danger" : undefined}
      >
        <div className={styles.cardBody}>
          <div className={styles.cardHead}>
            <Tooltip content={host.name}>
              <span className={styles.name}>
                <Text strong>{host.name}</Text>
              </span>
            </Tooltip>
            <StatusPill meta={meta} />
          </div>
          <Cluster gap={2}>
            <Tag>{hostKindMeta(host.kind).label}</Tag>
            <Version host={host} />
          </Cluster>
          <Seen host={host} />
          <Incidents host={host} />
        </div>
      </Card>
      <div className={styles.cardMenu}>
        <HostRowMenu host={host} />
      </div>
      {hasNoAgent(host) && <EnrollAction host={host} />}
    </div>
  );
}

/**
 * The hosts as cards, offline ones first: name and connection, kind and agent version, how
 * long it has been connected, and the open incidents. A card is one link to the host; its
 * menu sits over the corner as a sibling, so no control is nested in the link.
 */
export function HostCards({ hosts }: { hosts: readonly Host[] }) {
  const rows = useMemo(() => sortHosts(hosts, DEFAULT_HOST_SORT), [hosts]);
  return (
    <div className={styles.cards}>
      {rows.map((host) => (
        <HostCard key={host.id} host={host} />
      ))}
    </div>
  );
}
