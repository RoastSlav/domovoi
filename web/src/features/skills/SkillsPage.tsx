import { useHosts } from "../../api/hooks/hosts";
import { useSelectedHost } from "../../app/HostContext";
import { EmptyState, LoadError, PageHeader } from "../../components";
import ui from "../../components/ui.module.css";
import { SkillList } from "./SkillList";

/**
 * The skill catalogue of the host selected in the sidebar, grouped by risk. With "all hosts"
 * selected it shows the first host's skills, since a catalogue belongs to one host.
 */
export function SkillsPage() {
  const [selected] = useSelectedHost();
  const hosts = useHosts();
  const names = (hosts.data ?? []).map((h) => h.name);
  const host = selected ?? names[0] ?? "";

  return (
    <>
      <PageHeader
        title="Skills"
        docs="guide/skills.html"
        intro="Scripts an agent can run. Read skills only look, and triage may run them on its own. Write and destructive skills change things and need your approval. Disabled skills stay on the host but Domovoi never runs or suggests them."
      />
      {hosts.error && !hosts.data && (
        <LoadError error={hosts.error} context="load hosts" onRetry={hosts.refetch} />
      )}
      {hosts.data && names.length === 0 && (
        <EmptyState icon="server" title="No hosts yet">
          <p>Skills come from an agent. Add a host on the Hosts page first.</p>
        </EmptyState>
      )}
      {host && selected === undefined && (
        <p className={ui.muted}>
          Showing the skills of {host}. Choose a host in the sidebar to see another.
        </p>
      )}
      {host && <SkillList host={host} />}
    </>
  );
}
