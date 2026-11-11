import { Link } from "react-router";
import { useHosts } from "../../api/hooks/hosts";
import { useSelectedHost } from "../../app/HostContext";
import { EmptyState, PageHeader, QueryBoundary, Stack, StatusPill } from "../../components";
import { hostConnectionMeta, hostConnectionOf } from "../../design/tone";
import { SkillList } from "./SkillList";
import styles from "./skills.module.css";

/** Under "All hosts": the hosts, each a link that selects it and shows its skills. */
function ChooseHost() {
  const hosts = useHosts();
  return (
    <QueryBoundary
      query={hosts}
      context="load hosts"
      isEmpty={(list) => list.length === 0}
      empty={
        <EmptyState icon="server" title="No hosts yet">
          <p>Skills come from an agent. Add a host on the Hosts page first.</p>
        </EmptyState>
      }
    >
      {(list) => (
        <Stack gap={3}>
          <p>Choose a host to see its skills.</p>
          <ul className={styles.hostList} aria-label="Hosts">
            {list.map((host) => (
              <li key={host.id}>
                <Link to={`/skills?host=${encodeURIComponent(host.name)}`}>{host.name}</Link>
                <StatusPill size="sm" meta={hostConnectionMeta(hostConnectionOf(host))} />
              </li>
            ))}
          </ul>
        </Stack>
      )}
    </QueryBoundary>
  );
}

/**
 * The skill catalogue of the selected host, grouped by risk. A catalogue belongs to one
 * host, so under "All hosts" the page asks for one instead of guessing.
 */
export function SkillsPage() {
  const [selected] = useSelectedHost();
  return (
    <>
      <PageHeader
        title="Skills"
        docs="guide/skills.html"
        intro="Scripts an agent can run. Read skills only look, and triage may run them on its own. Write and destructive skills change things and need your approval. Disabled skills stay on the host but Domovoi never runs or suggests them."
      />
      {selected === undefined ? <ChooseHost /> : <SkillList host={selected} />}
    </>
  );
}
