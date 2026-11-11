import { Link } from "react-router";
import { useActions } from "../../api/hooks/actions";
import { useHosts } from "../../api/hooks/hosts";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Markdown } from "../../components/Markdown";
import { RelativeTime } from "../../components/RelativeTime";
import { riskMeta } from "../../design/tone";
import styles from "./skills.module.css";
import type { Skill } from "../../api/types";

/** A parameter's type and limits, from the JSON the API sends. */
export function kindText(kind: unknown): string {
  if (typeof kind !== "object" || kind === null) {
    return String(kind);
  }
  const { type, ...limits } = kind as Record<string, unknown>;
  const extra = Object.entries(limits)
    .map(([name, value]) => `${name} ${Array.isArray(value) ? value.join(" | ") : String(value)}`)
    .join(", ");
  return extra ? `${String(type)} (${extra})` : String(type);
}

/** How the hub checks the skill did its job, in one short phrase. */
const verificationText = (verify: Skill["verify"]) =>
  verify !== null && verify !== undefined && verify.recheck
    ? `verification re-runs the check after ${verify.delay_secs} s`
    : "no verification";

/** One skill in full: risk, parameter limits, verification, README and recent runs. */
export function SkillDetail({
  skill,
  host,
  onClose,
}: {
  host: string;
  onClose: () => void;
  skill: Skill;
}) {
  const actions = useActions();
  const hosts = useHosts();
  const hostId = (hosts.data ?? []).find((h) => h.name === host)?.id;
  const runs = (actions.data?.items ?? []).filter(
    (a) => a.host_id === hostId && a.skill === skill.name,
  );
  const hash = skill.manifest_hash;
  return (
    <Dialog
      title={skill.name}
      onClose={onClose}
      wide
      actions={<Button onClick={onClose}>Close</Button>}
    >
      <p className={styles.meta}>{`Risk ${skill.risk}: ${riskMeta(skill.risk).hint}`}</p>
      {skill.params.length > 0 && (
        <table className={styles.detail}>
          <caption>Parameters of {skill.name}</caption>
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Limits</th>
              <th scope="col">Required</th>
              <th scope="col">Description</th>
            </tr>
          </thead>
          <tbody>
            {skill.params.map((param) => (
              <tr key={param.name}>
                <td>
                  <code>{param.name}</code>
                </td>
                <td>{kindText(param.kind)}</td>
                <td>{param.required ? "required" : "optional"}</td>
                <td>{param.description}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className={styles.meta}>
        {skill.applies_to.length > 0 ? `Applies to ${skill.applies_to.join(", ")}` : "Any check"}
        {` · times out after ${skill.timeout_secs} s`}
        {` · ${verificationText(skill.verify)}`}
        {" · manifest hash "}
        <code title={hash}>{hash.slice(0, 12)}</code>
      </p>
      {skill.readme ? <Markdown text={skill.readme} /> : <p className={styles.meta}>No README</p>}
      <h3 className={styles.runsHead}>Recent runs on {host}</h3>
      {runs.length > 0 ? (
        <ul className={styles.runs}>
          {runs.slice(0, 10).map((run) => (
            <li key={run.id}>
              {run.status} · <RelativeTime at={run.updated_at} /> ·{" "}
              <Link to={`/incidents/${run.incident_id}`}>{run.incident_id}</Link>
            </li>
          ))}
        </ul>
      ) : (
        <p className={styles.meta}>No runs yet.</p>
      )}
    </Dialog>
  );
}
