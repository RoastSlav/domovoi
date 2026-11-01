import { useSearchParams } from "react-router";
import { useHosts } from "../../api/hooks/hosts";
import { EmptyState } from "../../components/EmptyState";
import { SkillList } from "./SkillList";
import styles from "./skills.module.css";

/** The skill catalogue of one host, chosen in the URL (`?host=`), grouped by risk. */
export function SkillsPage() {
  const [params, setParams] = useSearchParams();
  const hosts = useHosts();
  const names = (hosts.data ?? []).map((h) => h.name);
  const host = params.get("host") ?? names[0] ?? "";

  return (
    <>
      <h1>Skills</h1>
      {hosts.data && names.length === 0 && <EmptyState title="No hosts yet" />}
      {names.length > 0 && (
        <div className={styles.picker}>
          <label htmlFor="skills-host">Host</label>
          <select
            id="skills-host"
            value={host}
            onChange={(e) => setParams({ host: e.target.value })}
          >
            {names.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </div>
      )}
      {host && <SkillList host={host} />}
    </>
  );
}
