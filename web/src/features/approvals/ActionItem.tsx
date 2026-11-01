import { Link } from "react-router";
import type { Action } from "../../api/types";
import { RelativeTime } from "../../components/RelativeTime";
import styles from "./approvals.module.css";
import { ApproveControl } from "./ApproveControl";

/** An action with its risk, arguments and state, and Approve and Deny while proposed. */
export function ActionItem({
  action,
  host,
  showIncident = false,
}: {
  action: Action;
  host?: string | undefined;
  showIncident?: boolean;
}) {
  const args = Object.entries(action.args);
  return (
    <li className={styles.item}>
      <div className={styles.head}>
        <span className={styles.skill}>{action.skill}</span>
        <span className={styles.risk} data-risk={action.risk}>
          {action.risk}
        </span>
        <span>{action.status}</span>
        {host && <span className={styles.meta}>on {host}</span>}
      </div>
      {args.length > 0 && (
        <p className={styles.meta}>
          {args.map(([name, value]) => (
            <span key={name}>
              {name}=<code>{value}</code>{" "}
            </span>
          ))}
        </p>
      )}
      {action.reason && <p className={styles.meta}>{action.reason}</p>}
      <p className={styles.meta}>
        Proposed by {action.proposed_by} <RelativeTime at={action.created_at} />
        {action.status === "proposed" && (
          <>
            {" · expires "}
            <RelativeTime at={action.expires_at} />
          </>
        )}
        {showIncident && (
          <>
            {" · "}
            <Link to={`/incidents/${action.incident_id}`}>View incident</Link>
          </>
        )}
      </p>
      {action.status === "proposed" && <ApproveControl action={action} />}
    </li>
  );
}
