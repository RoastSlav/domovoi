import { Link } from "react-router";
import { useActions } from "../../api/hooks/actions";
import { useHostNames } from "../../api/hooks/hosts";
import { useIncidents } from "../../api/hooks/incidents";
import type { Action, IncidentSummary } from "../../api/types";
import {
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Section,
  Skeleton,
  StatusPill,
  Tooltip,
} from "../../components";
import { riskMeta, severityMeta } from "../../design/tone";
import styles from "./overview.module.css";

/** How many rows each list on the overview shows; the full list is one link away. */
export const PREVIEW_ROWS = 5;

/** The soonest to expire first, as on the Approvals page. */
function byExpiry(actions: readonly Action[]): Action[] {
  return [...actions].sort((a, b) => Date.parse(a.expires_at) - Date.parse(b.expires_at));
}

const SEVERITY_RANK: Record<string, number> = { critical: 0, warning: 1, info: 2 };

/** Most severe first, then the most recently fired. */
function bySeverity(incidents: readonly IncidentSummary[]): IncidentSummary[] {
  const rank = (incident: IncidentSummary) => SEVERITY_RANK[incident.severity] ?? 3;
  return [...incidents].sort(
    (a, b) => rank(a) - rank(b) || Date.parse(b.last_firing_at) - Date.parse(a.last_firing_at),
  );
}

/** A host name on one line, cut with an ellipsis; the whole name is its tooltip. */
function HostName({ name }: { name: string }) {
  return (
    <Tooltip content={name}>
      <span className={styles.host}>{name}</span>
    </Tooltip>
  );
}

function IncidentRow({ incident, host }: { incident: IncidentSummary; host: string | undefined }) {
  return (
    <li className={styles.item}>
      <Link to={`/incidents/${incident.id}`} className={styles.row}>
        <StatusPill size="sm" meta={severityMeta(incident.severity)} />
        <span className={styles.title}>{incident.title}</span>
        <HostName name={host ?? "Unknown host"} />
        <span className={styles.meta}>
          <RelativeTime at={incident.last_firing_at} />
        </span>
      </Link>
    </li>
  );
}

function ActionRow({ action, host }: { action: Action; host: string | undefined }) {
  return (
    <li className={styles.item}>
      <Link to="/approvals" className={styles.row}>
        <span className={styles.skill}>{action.skill}</span>
        <HostName name={host ?? "Unknown host"} />
        <StatusPill size="sm" meta={riskMeta(action.risk)} />
        <span className={styles.meta}>
          <RelativeTime at={action.created_at} />
        </span>
      </Link>
    </li>
  );
}

/** The first few incidents that need the person, each a link to its page. */
export function AttentionList({ host }: { host: string | undefined }) {
  const incidents = useIncidents({ host, group: "needs_action" });
  const names = useHostNames();
  return (
    <Section
      title="Needs attention"
      actions={
        <Link to="/incidents" className={styles.viewAll}>
          View all
        </Link>
      }
    >
      <QueryBoundary
        query={incidents}
        context="load incidents"
        loading={<Skeleton shape="rows" count={3} />}
        isEmpty={(data) => (data.pages[0]?.items.length ?? 0) === 0}
        empty={
          <EmptyState icon="check-circle" title="Nothing needs you">
            <p>No open incident needs action.</p>
          </EmptyState>
        }
      >
        {(data) => (
          <ul className={styles.list}>
            {bySeverity(data.pages[0]?.items ?? [])
              .slice(0, PREVIEW_ROWS)
              .map((incident) => (
                <IncidentRow
                  key={incident.id}
                  incident={incident}
                  host={names.get(incident.host_id)}
                />
              ))}
          </ul>
        )}
      </QueryBoundary>
    </Section>
  );
}

/** The first few actions waiting for a decision, the soonest to expire first. */
export function ApprovalList({ host }: { host: string | undefined }) {
  const actions = useActions({ status: "proposed", host });
  const names = useHostNames();
  return (
    <Section
      title="Waiting for approval"
      actions={
        <Link to="/approvals" className={styles.viewAll}>
          View all
        </Link>
      }
    >
      <QueryBoundary
        query={actions}
        context="load approvals"
        loading={<Skeleton shape="rows" count={3} />}
        isEmpty={(page) => page.items.length === 0}
        empty={
          <EmptyState icon="approve" title="Nothing to approve">
            <p>Actions triage suggests show up here.</p>
          </EmptyState>
        }
      >
        {(page) => (
          <ul className={styles.list}>
            {byExpiry(page.items)
              .slice(0, PREVIEW_ROWS)
              .map((action) => (
                <ActionRow key={action.id} action={action} host={names.get(action.host_id)} />
              ))}
          </ul>
        )}
      </QueryBoundary>
    </Section>
  );
}
