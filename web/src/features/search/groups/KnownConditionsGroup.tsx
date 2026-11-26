import { Link } from "react-router";
import { Highlight } from "../../../components";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Known conditions, matched in this browser against the complete list. */
export function KnownConditionsGroup({ q, state }: { q: string; state: SearchGroups["rules"] }) {
  return (
    <ResultGroup
      group={SEARCH_GROUPS[6]}
      state={state}
      rows={state.items.map((rule) => ({
        key: rule.id,
        node: (
          <>
            <span className={styles.line}>
              <Link to="/rules">
                <Highlight
                  text={`${rule.check}${rule.target_pattern ? ` on ${rule.target_pattern}` : ""}`}
                  query={q}
                />
              </Link>
            </span>
            <span className={styles.muted}>
              <Highlight text={rule.note} query={q} /> · {rule.host ?? "any host"}
            </span>
          </>
        ),
      }))}
    />
  );
}
