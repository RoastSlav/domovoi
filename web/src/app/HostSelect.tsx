import { useHosts } from "../api/hooks/hosts";
import type { Host } from "../api/types";
import { isHubOnly } from "../features/hosts/HostStatus";
import { useSelectedHost } from "./HostContext";
import styles from "./Layout.module.css";

/** An option's text: ● for a connected agent, ○ otherwise, with the reason when it is not. */
export function hostOptionLabel(host: Host): string {
  if (host.connected) {
    return `● ${host.name}`;
  }
  return `○ ${host.name} (${isHubOnly(host) ? "hub only" : "offline"})`;
}

/** The app-wide host choice: every page that filters by host follows it. */
export function HostSelect() {
  const hosts = useHosts();
  const [selected, select] = useSelectedHost();
  const list = hosts.data ?? [];
  // A selection the hub does not list (a stale link) still shows, so it can be seen and cleared.
  const unknown = selected !== undefined && !list.some((host) => host.name === selected);
  return (
    <select
      aria-label="Host"
      className={styles.hostSelect}
      value={selected ?? ""}
      onChange={(event) => select(event.target.value || undefined)}
    >
      <option value="">All hosts</option>
      {unknown && <option value={selected}>{selected}</option>}
      {list.map((host) => (
        <option key={host.id} value={host.name}>
          {hostOptionLabel(host)}
        </option>
      ))}
    </select>
  );
}
