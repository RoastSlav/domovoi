import type { Fact } from "../../api/types";
import { Section, Stack } from "../../components";
import { OverviewFacts } from "./OverviewFacts";
import { AllIncidentsLink, RecentIncidents } from "./OverviewIncidents";
import { OverviewSkills } from "./OverviewSkills";

/** The host's overview tab: a dashboard of the facts, the latest incidents and the skills it offers. */
export function HostOverview({ name, facts }: { name: string; facts: Fact[] }) {
  return (
    <Stack gap={5}>
      <OverviewFacts name={name} facts={facts} />
      <Section title="Recent incidents" actions={<AllIncidentsLink host={name} />}>
        <RecentIncidents host={name} />
      </Section>
      <Section title="Skills">
        <OverviewSkills host={name} />
      </Section>
    </Stack>
  );
}
