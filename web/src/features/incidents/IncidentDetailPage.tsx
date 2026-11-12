import { Link, useParams } from "react-router";
import { useIncident } from "../../api/hooks/incidents";
import { useHostNames } from "../../api/hooks/hosts";
import { ApiFailure, type Action } from "../../api/types";
import { usePageName } from "../../app/PageTitle";
import { useMediaQuery } from "../../app/useMediaQuery";
import {
  EmptyState,
  LoadError,
  PageHeader,
  Section,
  Skeleton,
  Stack,
  Text,
} from "../../components";
import { useActionOutcomes } from "../approvals/useActionOutcomes";
import { ActionSections } from "./ActionSections";
import { IncidentChat } from "./IncidentChat";
import { IncidentControls } from "./IncidentControls";
import { IncidentDetails } from "./IncidentDetails";
import { IncidentEvidence } from "./IncidentEvidence";
import { IncidentTimeline, ToolsUsed } from "./IncidentHistory";
import { IncidentMeta } from "./IncidentMeta";
import { ManualSteps } from "./ManualSteps";
import { MODEL_SETTINGS, useModelOff } from "./useModelOff";
import { useStatusNews } from "./useStatusNews";
import { WhatsGoingOn } from "./WhatsGoingOn";
import styles from "./incidents.module.css";

/** From this width the page has two columns: what is going on on the left, what to do on the right. */
export const TWO_COLUMN_QUERY = "(min-width: 1100px)";

const NO_ACTIONS: Action[] = [];

/** Where the chat box would be, when there is no model to ask: one line and the way to fix it. */
function NoModel({ line }: { line: string }) {
  return (
    <Text block muted>
      {line} <Link to={MODEL_SETTINGS}>Open model settings</Link>
    </Text>
  );
}

/**
 * One incident, readable in one screen: what is going on and why (triage's report), the
 * evidence, and what to approve. On a wide screen the story is on the left (report, evidence,
 * steps by hand, details, chat) and what to do and what happened on the right (actions to
 * decide, running and earlier actions, tools, timeline); the page scrolls as one. On a
 * narrow one the same parts follow each other in the order of `data-slot` in the CSS, with
 * what needs a decision straight after the report.
 */
export function IncidentDetailPage() {
  const { id = "" } = useParams();
  const incident = useIncident(id);
  const hosts = useHostNames();
  const wide = useMediaQuery(TWO_COLUMN_QUERY);
  const modelOff = useModelOff();
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

  const data = incident.data;
  const { card, report, actions, timeline, host, tools_used } = data;
  const summary = data.incident;
  // With a report its summary and cause are shown above; the card's details repeat them.
  const details = report ? report.details : card.details;
  // Acknowledge is the page's primary button while it is offered; Approve then steps back.
  const ackIsPrimary = summary.status !== "acked" && summary.status !== "resolved";
  return (
    <>
      <PageHeader
        title={summary.title}
        meta={<IncidentMeta detail={data} />}
        actions={<IncidentControls incident={summary} />}
      />
      <div className={styles.detail} data-columns={wide ? 2 : 1}>
        <div className={styles.main}>
          <div className={styles.slot} data-slot="going">
            <WhatsGoingOn detail={data} off={modelOff} />
          </div>
          <div className={styles.slot} data-slot="evidence">
            <IncidentEvidence detail={data} />
          </div>
          <div className={styles.slot} data-slot="byhand">
            <ManualSteps incidentId={summary.id} steps={report?.manual_steps ?? []} />
          </div>
          {details && (
            <div className={styles.slot} data-slot="details">
              <IncidentDetails text={details} />
            </div>
          )}
          <div className={styles.slot} data-slot="chat">
            <Section title="Chat">
              {modelOff ? (
                <NoModel line={modelOff.line} />
              ) : (
                <IncidentChat incidentId={summary.id} host={host} />
              )}
            </Section>
          </div>
        </div>

        <aside className={styles.side} aria-label="Actions and history">
          <ActionSections
            actions={actions}
            host={host}
            report={report}
            ackIsPrimary={ackIsPrimary}
          />
          <div className={styles.slot} data-slot="tools">
            <ToolsUsed tools={tools_used} />
          </div>
          <div className={styles.slot} data-slot="timeline">
            <IncidentTimeline entries={timeline} />
          </div>
        </aside>
      </div>
    </>
  );
}
