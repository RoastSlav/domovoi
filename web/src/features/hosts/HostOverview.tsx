import type { Fact, RetiredFact } from "../../api/types";
import { Section, Stack } from "../../components";
import { NoLongerReported } from "./NoLongerReported";
import { OverviewFacts } from "./OverviewFacts";
import { AllIncidentsLink, RecentIncidents } from "./OverviewIncidents";
import { OverviewSkills } from "./OverviewSkills";

/** The host's overview tab: a dashboard of the facts, the latest incidents and the skills it offers. */
export function HostOverview({
  name,
  facts,
  retired = [],
}: {
  name: string;
  facts: Fact[];
  retired?: readonly RetiredFact[];
}) {
  return (
    <Stack gap={5}>
      <OverviewFacts name={name} facts={facts} />
      <NoLongerReported facts={retired} />
      <Section title="Recent incidents" actions={<AllIncidentsLink host={name} />}>
        <RecentIncidents host={name} />
      </Section>
      <Section title="Skills">
        <OverviewSkills host={name} />
      </Section>
    </Stack>
  );
}
