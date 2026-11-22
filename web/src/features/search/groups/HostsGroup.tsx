import { Link } from "react-router";
import { Highlight, StatusPill } from "../../../components";
import { hostConnectionMeta, hostConnectionOf } from "../../../design/tone";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Hosts whose name matches, taken from the complete host list in this browser. */
export function HostsGroup({ q, state }: { q: string; state: SearchGroups["hosts"] }) {
  return (
    <ResultGroup
      group={SEARCH_GROUPS[5]}
      state={state}
      rows={state.items.map((host) => ({
        key: host.id,
        node: (
          <span className={styles.line}>
            <Link to={`/hosts/${encodeURIComponent(host.name)}`}>
              <Highlight text={host.name} query={q} />
            </Link>
            <StatusPill meta={hostConnectionMeta(hostConnectionOf(host))} size="sm" />
          </span>
        ),
      }))}
    />
  );
}
