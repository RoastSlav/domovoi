import { Link } from "react-router";
import { useHostNames } from "../../../api/hooks/hosts";
import { Highlight, RelativeTime, StatusPill } from "../../../components";
import { operationStatusMeta } from "../../../design/tone";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Operations that are not finished, matched in this browser against the newest of each state. */
export function ActiveOperationsGroup({
  q,
  state,
}: {
  q: string;
  state: SearchGroups["operations"];
}) {
  const names = useHostNames();
  return (
    <ResultGroup
      group={SEARCH_GROUPS[1]}
      state={state}
      rows={state.items.map((operation) => ({
        key: operation.id,
        node: (
          <>
            <span className={styles.line}>
              <StatusPill meta={operationStatusMeta(operation.status)} size="sm" />
              <Link to={`/operations/${encodeURIComponent(operation.id)}`}>
                <Highlight text={operation.goal} query={q} />
              </Link>
            </span>
            <span className={styles.muted}>
              {operation.hosts.map((host) => names.get(host) ?? host).join(", ")}
              {operation.hosts.length > 0 && " · "}
              <RelativeTime at={operation.updated_at} />
            </span>
          </>
        ),
      }))}
    />
  );
}
