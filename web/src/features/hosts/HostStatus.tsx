import type { Host } from "../../api/types";
import { RelativeTime } from "../../components/RelativeTime";
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
  return (
    <span className={styles.status} data-connected="false">
      Never connected
    </span>
  );
}

/** The agent's version, marked when it is older than the hub. */
export function VersionCell({ host }: { host: Host }) {
  if (!host.version) {
    return <span aria-label="unknown">–</span>;
  }
  return (
    <span className={styles.version}>
      {host.version}
      {host.outdated && (
        <span className={styles.outdated} title="Older than the hub; update the agent">
          Outdated
        </span>
      )}
    </span>
  );
}
