import { Link, useParams } from "react-router";
import { useIncident } from "../../api/hooks/incidents";
import { ApiFailure, type TimelineEntry } from "../../api/types";
import { EmptyState } from "../../components/EmptyState";
import { Markdown } from "../../components/Markdown";
import { RelativeTime } from "../../components/RelativeTime";
import ui from "../../components/ui.module.css";
import { ActionItem } from "../approvals/ActionItem";
import approvals from "../approvals/approvals.module.css";
import { IncidentChat } from "./IncidentChat";
import { IncidentCard } from "./IncidentCard";
import { IncidentControls } from "./IncidentControls";
import { RetryTriage } from "./RetryTriage";
import styles from "./incidents.module.css";

/** The timeline kind of a recorded follow-up question; the Chat section shows its text. */
const FOLLOW_UP_KIND = "triage.followup";

function Timeline({ entries }: { entries: TimelineEntry[] }) {
  if (entries.length === 0) {
    return <p>Nothing yet.</p>;
  }
  return (
    <ol className={styles.timeline}>
      {entries.map((entry, index) => (
        <li key={`${entry.at}-${index}`}>
          <RelativeTime at={entry.at} /> <span className={styles.kind}>{entry.kind}</span>{" "}
          {entry.kind === FOLLOW_UP_KIND ? "" : entry.summary}
        </li>
      ))}
    </ol>
  );
}

/** One incident: the card on top, then details, actions, the chat about it and history. */
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

  const { card, report, actions, timeline, host, tools_used } = incident.data;
  const summary = incident.data.incident;
  const manualSteps = report?.manual_steps ?? [];
  return (
    <>
      <p className={styles.meta}>
        <Link to="/incidents">Incidents</Link> · {host} · {summary.check}
        {summary.target ? ` ${summary.target}` : ""} · {summary.status}
      </p>
      <IncidentCard card={card} verdict={report?.verdict ?? summary.verdict ?? null} />

      {manualSteps.length > 0 && (
        <section className={styles.section} aria-labelledby="manual-heading">
          <h2 id="manual-heading">Check by hand</h2>
          <p className={styles.hint}>Written by the model; Domovoi does not run these.</p>
          <ol className={styles.checklist}>
            {manualSteps.map((step, index) => (
              <li key={`${step}-${index}`}>{step}</li>
            ))}
          </ol>
        </section>
      )}

      <RetryTriage incidentId={summary.id} triage={incident.data.triage} />
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

      <section className={styles.section} aria-labelledby="tools-heading">
        <h2 id="tools-heading">Skills and tools used</h2>
        <p className={styles.hint}>
          Read tools and read skills only look; they never change the host.
        </p>
        {tools_used.length === 0 && actions.length === 0 ? (
          <p>Nothing used yet.</p>
        ) : (
          <>
            {tools_used.length > 0 && (
              <div className={ui.tableWrap}>
                <table className={ui.table}>
                  <thead>
                    <tr>
                      <th scope="col">Name</th>
                      <th scope="col">Kind</th>
                      <th scope="col">Calls</th>
                      <th scope="col">Failures</th>
                      <th scope="col">Not run</th>
                      <th scope="col">Last used</th>
                    </tr>
                  </thead>
                  <tbody>
                    {tools_used.map((tool) => (
                      <tr key={`${tool.kind}-${tool.name}`}>
                        <td>{tool.name}</td>
                        <td>{tool.kind}</td>
                        <td>{tool.calls}</td>
                        <td>{tool.failures}</td>
                        <td>{tool.refused}</td>
                        <td>
                          <RelativeTime at={tool.last_used} />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {actions.length > 0 && (
              <p>
                Skills proposed: {[...new Set(actions.map((action) => action.skill))].join(", ")}.
              </p>
            )}
          </>
        )}
      </section>

      <section className={styles.section} aria-labelledby="chat-heading">
        <h2 id="chat-heading">Chat</h2>
        <IncidentChat incidentId={summary.id} host={host} />
      </section>

      <section className={styles.section} aria-labelledby="timeline-heading">
        <h2 id="timeline-heading">Timeline</h2>
        <p className={styles.hint}>Everything that happened to this incident, oldest first.</p>
        <Timeline entries={timeline} />
      </section>
    </>
  );
}
