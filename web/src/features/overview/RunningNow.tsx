import { Link } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { useOperations } from "../../api/hooks/operations";
import { EmptyState, QueryBoundary, RelativeTime, Skeleton, StatusPill } from "../../components";
import { operationStatusMeta } from "../../design/tone";
import { PREVIEW_ROWS } from "../attention/AttentionRows";
import { hostLabel } from "../attention/AttentionRow";
import attention from "../attention/attention.module.css";

/** The operations that are running now: the first few, each a link to its page. */
export function RunningNow({ host }: { host: string | undefined }) {
  const running = useOperations({ status: "running", host });
  const names = useHostNames();
  return (
    <QueryBoundary
      query={running}
      context="load running operations"
      loading={<Skeleton shape="rows" count={3} />}
      isEmpty={(data) => (data.pages[0]?.items.length ?? 0) === 0}
      empty={
        <EmptyState icon="play" title="Nothing is running">
          <p>Operations appear here while Domovoi investigates or fixes something.</p>
        </EmptyState>
      }
    >
      {(data) => (
        <ul className={attention.list}>
          {(data.pages[0]?.items ?? []).slice(0, PREVIEW_ROWS).map((op) => (
            <li key={op.id} className={attention.item}>
              <Link to={`/operations/${op.id}`} className={attention.row}>
                <StatusPill size="sm" meta={operationStatusMeta(op.status)} />
                <span className={attention.main}>
                  <span className={attention.title}>{op.goal}</span>
                </span>
                <span className={attention.trail}>
                  <span className={attention.host}>
                    {hostLabel(op.hosts.map((id) => names.get(id) ?? id))}
                  </span>
                  <span className={attention.meta}>
                    <RelativeTime at={op.updated_at} />
                  </span>
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </QueryBoundary>
  );
}
