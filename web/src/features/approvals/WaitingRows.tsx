import { useState } from "react";
import type { Action } from "../../api/types";
import { Button, IconButton, StatusPill, Tooltip } from "../../components";
import { riskMeta } from "../../design/tone";
import { Expiry } from "./ActionCard";
import { ApproveControl } from "./ApproveControl";
import { WaitingCard } from "./WaitingCard";
import styles from "./approvals.module.css";

/** The arguments on one line, for the row: `name=web volume=old`. */
function summary(action: Action): string {
  return Object.entries(action.args)
    .map(([name, value]) => `${name}=${value}`)
    .join(" ");
}

/**
 * One waiting action as one line: skill, host, risk, a summary of the arguments, the time
 * left, and Approve and Deny (the same controls as the card, so a destructive action still
 * asks in its dialog). The chevron opens it in place to the full card with the arguments, why
 * it was proposed and the incident; "Show less" folds it back.
 */
function WaitingRow({ action, host }: { action: Action; host: string | undefined }) {
  const [open, setOpen] = useState(false);
  if (open) {
    return (
      <li className={styles.rowOpen}>
        <WaitingCard action={action} host={host} />
        <Button size="sm" variant="ghost" icon="chevron-up" onClick={() => setOpen(false)}>
          Show less
        </Button>
      </li>
    );
  }
  const name = host ?? "Unknown host";
  return (
    <li className={styles.row}>
      <IconButton
        icon="chevron-right"
        size="sm"
        label={`Show details of ${action.skill}`}
        aria-expanded={false}
        onClick={() => setOpen(true)}
      />
      <span className={styles.rowSkill}>{action.skill}</span>
      <Tooltip content={name}>
        <span className={styles.rowHost}>{name}</span>
      </Tooltip>
      <StatusPill size="sm" meta={riskMeta(action.risk)} />
      <span className={styles.rowArgs}>{summary(action)}</span>
      <Expiry until={action.expires_at} />
      <ApproveControl action={action} host={host} emphasis="primary" compact />
    </li>
  );
}

/** Many waiting actions as one bordered list of compact rows, the soonest to expire first. */
export function WaitingRows({
  actions,
  hosts,
}: {
  actions: readonly Action[];
  hosts: Map<string, string>;
}) {
  return (
    <ul className={styles.rows} aria-label="Waiting actions">
      {actions.map((action) => (
        <WaitingRow key={action.id} action={action} host={hosts.get(action.host_id)} />
      ))}
    </ul>
  );
}
