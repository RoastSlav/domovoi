import { useState } from "react";

import { useSetSkillEnabled, useSkills } from "../../api/hooks/hosts";
import { EmptyState, QueryBoundary, Skeleton, useActionFeedback } from "../../components";
import { SkillDetail } from "./SkillDetail";
import { SkillTable } from "./SkillTable";

/** Shown when a host's agent reported no skills: why, and what to check. */
function NoSkills({ host }: { host: string }) {
  return (
    <EmptyState icon="tool" title="No skills">
      <p>{`${host} has not reported any skills. Its agent sends them when it connects, so check that ${host} is connected on the Hosts page.`}</p>
    </EmptyState>
  );
}

/** The skills `host`'s agent reported; choosing a name opens its details. */
export function SkillList({ host }: { host: string }) {
  const [opened, setOpened] = useState<string | null>(null);
  const skills = useSkills(host);
  const raw = useSetSkillEnabled(host);
  const toggle = useActionFeedback(raw, {
    success: (_, change) => `${change.skill} ${change.enabled ? "enabled" : "disabled"}`,
    error: "change the skill",
    undo: (_, change) => void raw.mutateAsync({ ...change, enabled: !change.enabled }),
  });
  return (
    <>
      <QueryBoundary
        query={skills}
        context="load skills"
        loading={<Skeleton shape="rows" />}
        isEmpty={(list) => list.length === 0}
        empty={<NoSkills host={host} />}
      >
        {(list) => (
          <SkillTable
            host={host}
            skills={list}
            onOpen={(skill) => setOpened(skill.name)}
            onToggle={(skill, enabled) => toggle.mutate({ enabled, skill: skill.name })}
          />
        )}
      </QueryBoundary>
      {opened !== null && <SkillDetail host={host} name={opened} onClose={() => setOpened(null)} />}
    </>
  );
}
