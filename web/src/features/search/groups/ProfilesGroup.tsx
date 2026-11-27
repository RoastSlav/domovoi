import { Link } from "react-router";
import { Highlight } from "../../../components";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Host profiles, matched in this browser against each host's stored profile text. */
export function ProfilesGroup({ q, state }: { q: string; state: SearchGroups["profiles"] }) {
  return (
    <ResultGroup
      group={SEARCH_GROUPS[8]}
      state={state}
      rows={state.items.map((hit) => ({
        key: hit.host,
        node: (
          <>
            <span className={styles.line}>
              <Link to={`/hosts/${encodeURIComponent(hit.host)}`}>{hit.host}</Link>
            </span>
            <span className={styles.muted}>
              <Highlight text={hit.line} query={q} />
            </span>
          </>
        ),
      }))}
    />
  );
}
