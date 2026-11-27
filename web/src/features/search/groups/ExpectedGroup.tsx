import { Link } from "react-router";
import { Highlight } from "../../../components";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Events a known condition explained: history with the reason, never an incident. */
export function ExpectedGroup({ q, state }: { q: string; state: SearchGroups["expected"] }) {
  return (
    <ResultGroup
      group={SEARCH_GROUPS[7]}
      state={state}
      rows={state.items.map((row) => ({
        key: row.id,
        node: (
          <>
            <span className={styles.line}>
              <Link to={`/hosts/${encodeURIComponent(row.host)}?tab=expected`}>
                <Highlight text={row.summary} query={q} />
              </Link>
            </span>
            <span className={styles.muted}>
              {row.host} · <Highlight text={row.rule_note ?? "known condition removed"} query={q} />
            </span>
          </>
        ),
      }))}
    />
  );
}
