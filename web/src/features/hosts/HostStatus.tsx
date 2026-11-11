import { useState } from "react";
import type { Host } from "../../api/types";
import { Button, RelativeTime, Tooltip } from "../../components";
import { hostConnectionMeta } from "../../design/tone";
import { HowToUpdateDialog } from "../updates/HowToUpdateDialog";
import styles from "./hosts.module.css";

/** Whether a host's agent is connected, and since or until when. */
export function HostStatus({ host }: { host: Host }) {
  if (host.connected) {
    return (
      <span className={styles.status} data-connected="true">
        Connected
        {host.connected_since && (
          <>
            {" since "}
            <RelativeTime at={host.connected_since} />
          </>
        )}
      </span>
    );
  }
  if (host.last_seen) {
    return (
      <span className={styles.status} data-connected="false">
        Offline, last seen <RelativeTime at={host.last_seen} />
      </span>
    );
  }
  if (isHubOnly(host)) {
    return (
      <Tooltip content={hostConnectionMeta("hub_only").hint}>
        <span className={styles.status} data-connected="hub">
          Hub only, no agent
        </span>
      </Tooltip>
    );
  }
  return (
    <span className={styles.status} data-connected="false">
      Never connected
    </span>
  );
}

/** Whether the host is the hub's own machine and no agent was ever enrolled on it.
 * The hub keeps its own host as `local`; enrolled agents survive restarts, so the
 * missing flag, not the lost presence, marks the hub-only state. */
export function isHubOnly(host: Host): boolean {
  return host.kind === "local" && !host.agent_enrolled;
}

/** The agent's version, marked when it is older than the hub, or why it was refused. */
export function VersionCell({ host }: { host: Host }) {
  const [updating, setUpdating] = useState(false);
  const outdated = host.outdated;
  if (host.refused) {
    return (
      <Tooltip content={hostConnectionMeta("refused").hint}>
        <span className={styles.refused}>{host.refused}</span>
      </Tooltip>
    );
  }
  if (!host.version) {
    return <span aria-label="unknown">–</span>;
  }
  return (
    <>
      <span className={styles.version}>
        {host.version}
        {outdated && (
          <Tooltip content="Older than the hub; update the agent.">
            <span className={styles.outdated}>Outdated</span>
          </Tooltip>
        )}
        {outdated && (
          <Button size="sm" variant="ghost" onClick={() => setUpdating(true)}>
            How to update
          </Button>
        )}
      </span>
      {updating && outdated && <HowToUpdateDialog onClose={() => setUpdating(false)} />}
    </>
  );
}
