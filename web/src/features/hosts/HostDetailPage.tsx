import { Link, useParams } from "react-router";
import { useHost } from "../../api/hooks/hosts";
import { useIncidents } from "../../api/hooks/incidents";
import { ApiFailure, type Fact } from "../../api/types";
import { Badge } from "../../components/Badge";
import { EmptyState } from "../../components/EmptyState";
import { RelativeTime } from "../../components/RelativeTime";
import { SkillList } from "../skills/SkillList";
import { HostStatus, VersionCell } from "./HostStatus";
import styles from "./hosts.module.css";

function Facts({ facts }: { facts: Fact[] }) {
  if (facts.length === 0) {
    return <p>No facts reported in the last day.</p>;
  }
  return (
    <dl className={styles.facts}>
      {facts.map((fact) => (
        <div key={fact.key} className={styles.fact}>
          <dt>{fact.label}</dt>
          <dd>
            {fact.value}
            {fact.unit ? ` ${fact.unit}` : ""}{" "}
            <small>
              (<RelativeTime at={fact.observed_at} />)
            </small>
          </dd>
        </div>
      ))}
    </dl>
  );
}

function RecentIncidents({ host }: { host: string }) {
  const incidents = useIncidents({ host });
  const items = incidents.data?.pages[0]?.items.slice(0, 10) ?? [];
  if (incidents.isPending) {
    return <p>Loading…</p>;
  }
  if (items.length === 0) {
    return <p>No incidents.</p>;
  }
  return (
    <ul>
      {items.map((incident) => (
        <li key={incident.id}>
          <Badge value={incident.verdict ?? incident.severity} />{" "}
          <Link to={`/incidents/${incident.id}`}>{incident.title}</Link> · {incident.status} ·{" "}
          <RelativeTime at={incident.last_firing_at} />
        </li>
      ))}
    </ul>
  );
}

/** One host: its agent, the latest digest facts, recent incidents and skills. */
export function HostDetailPage() {
  const { name = "" } = useParams();
  const host = useHost(name);

  if (host.isPending) {
    return <p>Loading…</p>;
  }
  if (host.error || !host.data) {
    const missing = host.error instanceof ApiFailure && host.error.status === 404;
    return (
      <EmptyState title={missing ? "No such host" : "Could not load the host"}>
        <Link to="/hosts">Back to hosts</Link>
      </EmptyState>
    );
  }
  const detail = host.data;
  return (
    <>
      <p>
        <Link to="/hosts">Hosts</Link>
      </p>
      <h1>{detail.host.name}</h1>
      <p>
        <HostStatus host={detail.host} /> · agent <VersionCell host={detail.host} /> ·{" "}
        {detail.host.kind}
      </p>

      <section aria-labelledby="facts-heading">
        <h2 id="facts-heading">Facts</h2>
        <Facts facts={detail.facts} />
      </section>

      <section aria-labelledby="incidents-heading">
        <h2 id="incidents-heading">Recent incidents</h2>
        <RecentIncidents host={detail.host.name} />
        <p>
          <Link to={`/incidents?host=${encodeURIComponent(detail.host.name)}`}>
            All incidents on {detail.host.name}
          </Link>
        </p>
      </section>

      <section aria-labelledby="skills-heading">
        <h2 id="skills-heading">Skills</h2>
        <SkillList host={detail.host.name} />
      </section>
    </>
  );
}
