import type { Fact } from "../../api/types";
import { Section, Stack } from "../../components";
import { OverviewFacts } from "./OverviewFacts";
import { AllIncidentsLink, RecentIncidents } from "./OverviewIncidents";
import { OverviewSkills } from "./OverviewSkills";

/** The host's overview tab: facts by topic, the latest incidents and the skills it offers. */
export function HostOverview({ name, facts }: { name: string; facts: Fact[] }) {
  return (
    <Stack gap={5}>
      <Section title="Facts" description="The latest value of each measurement from the last day.">
        <OverviewFacts name={name} facts={facts} />
      </Section>
      <Section title="Recent incidents" actions={<AllIncidentsLink host={name} />}>
        <RecentIncidents host={name} />
      </Section>
      <Section title="Skills">
        <OverviewSkills host={name} />
      </Section>
    </Stack>
  );
}
