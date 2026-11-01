import { useSkills } from "../../api/hooks/hosts";
import type { Skill } from "../../api/types";
import { EmptyState } from "../../components/EmptyState";
import styles from "./skills.module.css";

const GROUPS = [
  ["read", "Read"],
  ["write", "Write"],
  ["destructive", "Destructive"],
] as const;

/** A parameter's type and limits, from the JSON the API sends. */
function kindText(kind: unknown): string {
  if (typeof kind !== "object" || kind === null) {
    return String(kind);
  }
  const { type, ...limits } = kind as Record<string, unknown>;
  const extra = Object.entries(limits)
    .map(([name, value]) => `${name} ${Array.isArray(value) ? value.join(" | ") : String(value)}`)
    .join(", ");
  return extra ? `${String(type)} (${extra})` : String(type);
}

function SkillCard({ skill }: { skill: Skill }) {
  const destructive = skill.risk === "destructive";
  return (
    <li className={styles.skill} data-risk={skill.risk}>
      <h4 className={styles.name}>
        {skill.name}
        {destructive && <span className={styles.danger}>Destructive</span>}
      </h4>
      <p>{skill.description}</p>
      {skill.params.length > 0 && (
        <ul className={styles.params} aria-label={`Parameters of ${skill.name}`}>
          {skill.params.map((param) => (
            <li key={param.name}>
              <code>{param.name}</code> {kindText(param.kind)}
              {param.required ? ", required" : ", optional"}
              {param.description && ` — ${param.description}`}
            </li>
          ))}
        </ul>
      )}
      <p className={styles.meta}>
        {skill.applies_to.length > 0 ? `Applies to ${skill.applies_to.join(", ")}` : "Any check"}
        {` · times out after ${skill.timeout_secs} s`}
      </p>
    </li>
  );
}

/** The skills `host`'s agent reported, grouped by risk. */
export function SkillList({ host }: { host: string }) {
  const skills = useSkills(host);
  if (skills.isPending) {
    return <p>Loading…</p>;
  }
  if (skills.error) {
    return <EmptyState title="Could not load skills" />;
  }
  if (skills.data.length === 0) {
    return (
      <EmptyState title="No skills">
        <p>The agent reports its skills when it connects.</p>
      </EmptyState>
    );
  }
  return (
    <>
      {GROUPS.map(([risk, label]) => {
        const group = skills.data.filter((s) => s.risk === risk);
        if (group.length === 0) {
          return null;
        }
        return (
          <section key={risk} aria-label={`${label} skills`} className={styles.group}>
            <h3>
              {label} <span className={styles.count}>({group.length})</span>
            </h3>
            <ul className={styles.list}>
              {group.map((skill) => (
                <SkillCard key={skill.name} skill={skill} />
              ))}
            </ul>
          </section>
        );
      })}
    </>
  );
}
