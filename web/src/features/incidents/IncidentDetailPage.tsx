import { Link, useParams } from "react-router";
import { useIncident } from "../../api/hooks/incidents";
import { useHostNames } from "../../api/hooks/hosts";
import { ApiFailure, type Action, type IncidentDetail, type Report } from "../../api/types";
import { usePageName } from "../../app/PageTitle";
import { useMediaQuery } from "../../app/useMediaQuery";
import {
  EmptyState,
  LoadError,
  Markdown,
  PageHeader,
  RelativeTime,
  Section,
  Skeleton,
  Stack,
  StatusPill,
  Text,
} from "../../components";
import { incidentStatusMeta, severityMeta, verdictMeta } from "../../design/tone";
import { ActionCard } from "../approvals/ActionCard";
import { useActionOutcomes } from "../approvals/useActionOutcomes";
import { IncidentCard } from "./IncidentCard";
import { IncidentChat } from "./IncidentChat";
import { IncidentControls } from "./IncidentControls";
import { IncidentTimeline, ToolsUsed } from "./IncidentHistory";
import { ManualSteps } from "./ManualSteps";
import { TriagePanel } from "./TriagePanel";
import { useStatusNews } from "./useStatusNews";
import styles from "./incidents.module.css";

/** From this width the page has two columns: the story on the left, what to do on the right. */
export const TWO_COLUMN_QUERY = "(min-width: 1100px)";

const NO_ACTIONS: Action[] = [];

/** Why the model proposed `action`: the reason of the report's proposal with the same skill and arguments. */
function whyProposed(action: Action, report: Report | null | undefined): string | undefined {
  const args = JSON.stringify(action.args);
  return report?.proposals.find(
    (proposal) => proposal.skill === action.skill && JSON.stringify(proposal.args) === args,
  )?.reason;
}

/** The facts under the title: where it stands, how bad, where and since when. */
function Facts({ detail }: { detail: IncidentDetail }) {
  const { incident, host, report } = detail;
  const verdict = report?.verdict ?? incident.verdict;
  return (
    <>
      <StatusPill meta={incidentStatusMeta(incident.status)} />
      {verdict && <StatusPill meta={verdictMeta(verdict)} />}
      <StatusPill meta={severityMeta(incident.severity)} />
      <Link to={`/hosts/${encodeURIComponent(host)}`}>{host}</Link>
      <Text mono size="sm">
        {incident.target ? `${incident.check} ${incident.target}` : incident.check}
      </Text>
      <Text muted size="sm">
        Opened <RelativeTime at={incident.opened_at} />
      </Text>
      <Text muted size="sm">
        Last fired <RelativeTime at={incident.last_firing_at} />
      </Text>
      {incident.occurrences > 1 && (
        <Text muted size="sm">
          {incident.occurrences} times
        </Text>
      )}
    </>
  );
}

/**
 * One incident. On a wide screen the story is on the left (summary, triage, steps by hand,
 * details, chat) and what to do is on the right (proposed actions, tools, timeline). On a
 * narrow one it is a single column with the proposed actions straight after the summary:
 * the next step comes first on a phone.
 */
export function IncidentDetailPage() {
  const { id = "" } = useParams();
  const incident = useIncident(id);
  const hosts = useHostNames();
  const wide = useMediaQuery(TWO_COLUMN_QUERY);
  usePageName(incident.data?.incident.title);
  useStatusNews(id, incident.data?.incident.status);
  useActionOutcomes(incident.data?.actions ?? NO_ACTIONS, hosts);

  if (!incident.data) {
    if (incident.error instanceof ApiFailure && incident.error.status === 404) {
      return (
        <EmptyState
          title="No such incident"
          action={<Link to="/incidents">Back to incidents</Link>}
        >
          <p>It may have been removed, or the address is wrong.</p>
        </EmptyState>
      );
    }
    if (incident.error) {
      return (
        <LoadError error={incident.error} context="load the incident" onRetry={incident.refetch} />
      );
    }
    return (
      <Stack gap={4}>
        <span className="visually-hidden" role="status">
          Loading…
        </span>
        <Skeleton shape="line" count={2} />
        <Skeleton shape="block" />
        <Skeleton shape="rows" />
      </Stack>
    );
  }

  const { card, report, actions, timeline, host, tools_used } = incident.data;
  const summary = incident.data.incident;
  const details = card.details ?? report?.details;
  return (
    <>
      <PageHeader
        title={summary.title}
        meta={<Facts detail={incident.data} />}
        actions={<IncidentControls incident={summary} />}
      />
      <div className={styles.detail} data-columns={wide ? 2 : 1}>
        <div className={styles.summary}>
          <IncidentCard card={card} verdict={report?.verdict ?? summary.verdict ?? null} />
        </div>

        <aside className={styles.side} aria-label="Actions and history">
          <div className={styles.actions}>
            <Section title="Proposed actions">
              {actions.length === 0 ? (
                <Text block muted size="sm">
                  No actions proposed. Nothing runs on a host unless you approve it here.
                </Text>
              ) : (
                <Stack gap={3}>
                  {actions.map((action) => (
                    <ActionCard
                      key={action.id}
                      action={action}
                      host={host}
                      why={whyProposed(action, report)}
                    />
                  ))}
                </Stack>
              )}
            </Section>
          </div>
          <div className={styles.history}>
            <ToolsUsed
              tools={tools_used}
              proposed={[...new Set(actions.map((action) => action.skill))]}
            />
            <IncidentTimeline entries={timeline} />
          </div>
        </aside>

        <div className={styles.main}>
          <TriagePanel incidentId={summary.id} triage={incident.data.triage} />
          <ManualSteps incidentId={summary.id} steps={report?.manual_steps ?? []} />
          {details && (
            <Section title="Details">
              <Markdown text={details} />
            </Section>
          )}
          <Section title="Chat">
            <IncidentChat incidentId={summary.id} host={host} />
          </Section>
        </div>
      </div>
    </>
  );
}
