import { Link } from "react-router";
import { useHostNames } from "../../../api/hooks/hosts";
import { Highlight, RelativeTime, StatusPill, Tag, Visible } from "../../../components";
import { operationStatusMeta } from "../../../design/tone";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Finished operations the hub remembers. Read-only: the hub's label says what they are. */
export function PastOperationsGroup({
  q,
  state,
  label,
}: {
  label: string | undefined;
  q: string;
  state: SearchGroups["past"];
}) {
  const names = useHostNames();
  return (
    <ResultGroup
      group={SEARCH_GROUPS[2]}
      state={state}
      lead={
        label ? (
          <Visible category="P0" id="search.history-label">
            <p className={styles.muted}>{label}</p>
          </Visible>
        ) : undefined
      }
      rows={state.items.map(({ operation, snippet }) => ({
        key: operation.operation_id,
        node: (
          <>
            <span className={styles.line}>
              <StatusPill meta={operationStatusMeta(operation.outcome)} size="sm" />
              <Link to={`/operations/${encodeURIComponent(operation.operation_id)}`}>
                <Highlight text={operation.goal} query={q} />
              </Link>
              {operation.verified && <Tag>Verified</Tag>}
            </span>
            <span className={styles.muted}>
              {operation.hosts.map((host) => host.name ?? names.get(host.id) ?? host.id).join(", ")}
              {operation.hosts.length > 0 && " · "}
              <RelativeTime at={operation.ended_at} />
            </span>
            {snippet && (
              <span>
                <Highlight text={snippet} query={q} />
              </span>
            )}
          </>
        ),
      }))}
    />
  );
}
