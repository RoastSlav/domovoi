import { Link, useParams } from "react-router";
import { useIncident } from "../../api/hooks/incidents";
import { ApiFailure, type TimelineEntry } from "../../api/types";
import { EmptyState } from "../../components/EmptyState";
import { Markdown } from "../../components/Markdown";
import { RelativeTime } from "../../components/RelativeTime";
import { ActionItem } from "../approvals/ActionItem";
import approvals from "../approvals/approvals.module.css";
import { AskBox } from "./AskBox";
import { IncidentCard } from "./IncidentCard";
import { IncidentControls } from "./IncidentControls";
import styles from "./incidents.module.css";

function Timeline({ entries }: { entries: TimelineEntry[] }) {
  if (entries.length === 0) {
    return <p>Nothing yet.</p>;
  }
  return (
    <ol className={styles.timeline}>
      {entries.map((entry, index) => (
        <li key={`${entry.at}-${index}`}>
          <RelativeTime at={entry.at} /> <span className={styles.kind}>{entry.kind}</span>{" "}
          {entry.summary}
        </li>
      ))}
    </ol>
  );
}

/** One incident: the card on top, then details, actions, follow-up questions and history. */
export function IncidentDetailPage() {
  const { id = "" } = useParams();
  const incident = useIncident(id);

  if (incident.isPending) {
    return <p>Loading…</p>;
  }
  if (incident.error || !incident.data) {
    const missing = incident.error instanceof ApiFailure && incident.error.status === 404;
    return (
      <EmptyState title={missing ? "No such incident" : "Could not load the incident"}>
        <Link to="/incidents">Back to incidents</Link>
      </EmptyState>
    );
  }

  const { card, report, actions, timeline, host } = incident.data;
  const summary = incident.data.incident;
  return (
    <>
      <p className={styles.meta}>
        <Link to="/incidents">Incidents</Link> · {host} · {summary.check}
        {summary.target ? ` ${summary.target}` : ""} · {summary.status}
      </p>
      <IncidentCard card={card} />
      <IncidentControls incident={summary} />

      {(card.details ?? report?.details) && (
        <section className={styles.section} aria-labelledby="details-heading">
          <h2 id="details-heading">Details</h2>
          <Markdown text={card.details ?? report?.details ?? ""} />
        </section>
      )}

      <section className={styles.section} aria-labelledby="actions-heading">
        <h2 id="actions-heading">Actions</h2>
        {actions.length === 0 ? (
          <p>No actions proposed.</p>
        ) : (
          <ul className={approvals.list}>
            {actions.map((action) => (
              <ActionItem key={action.id} action={action} />
            ))}
          </ul>
        )}
      </section>

      <section className={styles.section} aria-labelledby="ask-heading">
        <h2 id="ask-heading">Follow-up</h2>
        <AskBox incidentId={summary.id} />
      </section>

      <section className={styles.section} aria-labelledby="timeline-heading">
        <h2 id="timeline-heading">Timeline</h2>
        <Timeline entries={timeline} />
      </section>
    </>
  );
}
