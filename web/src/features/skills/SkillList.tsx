import { useState } from "react";

import { useCapabilities } from "../../api/hooks/capabilities";
import { useSetSkillEnabled, useSkills } from "../../api/hooks/hosts";
import {
  EmptyState,
  InlineMessage,
  QueryBoundary,
  Skeleton,
  useActionFeedback,
} from "../../components";
import { runnerSentence } from "../capabilities/capabilityText";
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
  // The Capabilities page says why skills cannot run on a host without a skill runner; say
  // it here too, where a person turns them on.
  const capability = useCapabilities(host).data?.entries[0];
  const cannotRun =
    capability !== undefined && !capability.runner_installed && !capability.root_agent;
  return (
    <>
      {cannotRun && (
        <InlineMessage tone="warning" title="Skills cannot run on this host yet">
          <p>{runnerSentence(capability)}</p>
          <p>
            You can still enable skills, but every run is refused until the agent has a skill
            runner. The Capabilities page has the details.
          </p>
        </InlineMessage>
      )}
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
