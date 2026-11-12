import { useHostNames } from "../../../api/hooks/hosts";
import { useAdvisories } from "../../../api/hooks/system";
import { EmptyState, QueryBoundary, RelativeTime, Skeleton, Tag } from "../../../components";
import styles from "../settings.module.css";

/** The open advisories as one compact bordered list: title, host, time, then the advice. */
export function Advisories() {
  const advisories = useAdvisories();
  const hosts = useHostNames();
  return (
    <QueryBoundary
      query={advisories}
      context="load advisories"
      loading={<Skeleton shape="rows" count={2} />}
      isEmpty={(list) => list.length === 0}
      empty={
        <EmptyState icon="check-circle" title="No open advisories">
          <p>Nothing the hub has noticed needs your attention.</p>
        </EmptyState>
      }
    >
      {(list) => (
        <ul className={styles.list}>
          {list.map((advisory) => (
            <li key={advisory.id} className={styles.listRow}>
              <div className={styles.listHead}>
                <span className={styles.listTitle}>{advisory.title}</span>
                {advisory.host_id && <Tag>{hosts.get(advisory.host_id) ?? "unknown host"}</Tag>}
                <span className={styles.listTime}>
                  <RelativeTime at={advisory.created_at} />
                </span>
              </div>
              <p className={styles.listAdvice}>{advisory.detail}</p>
            </li>
          ))}
        </ul>
      )}
    </QueryBoundary>
  );
}
