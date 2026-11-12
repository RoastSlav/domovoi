import type { Fact } from "../../api/types";
import { EmptyState, Grid, KeyValueList, Section, Stack } from "../../components";
import { withUnit } from "./factValue";
import { Stale, Updated } from "./Freshness";
import { OverviewContainers } from "./OverviewContainers";
import { OverviewDisks } from "./OverviewDisks";
import { OverviewHealth } from "./OverviewHealth";
import { OverviewPanel } from "./OverviewPanel";
import { splitOverview } from "./splitOverview";

/** Facts as a label and value list, each value with a mark when it has gone stale. */
function FactList({ facts }: { facts: readonly Fact[] }) {
  return (
    <KeyValueList
      items={facts.map((fact) => ({
        label: fact.label,
        value: (
          <>
            {withUnit(fact)}
            <Stale at={fact.observed_at} />
          </>
        ),
      }))}
    />
  );
}

/**
 * What the host's checks measured, as a dashboard: health tiles, disks, containers, the
 * network and, behind a disclosure, every other fact. Each part says once when it was last
 * updated and marks only the facts that have gone stale. A host with no facts says what to
 * check.
 */
export function OverviewFacts({ name, facts }: { facts: Fact[]; name: string }) {
  if (facts.length === 0) {
    return (
      <EmptyState icon="info" title="No facts in the last day">
        <p>Check that the agent on {name} is connected and that its checks are on.</p>
      </EmptyState>
    );
  }
  const overview = splitOverview(facts);
  return (
    <Stack gap={5}>
      <OverviewHealth tiles={overview.health} />
      <Grid min="lg">
        <OverviewDisks rows={overview.disks} lines={overview.storage} />
        {overview.network.length > 0 && (
          <OverviewPanel title="Network" updated={<Updated facts={overview.network} />}>
            <FactList facts={overview.network} />
          </OverviewPanel>
        )}
      </Grid>
      <OverviewContainers host={name} rows={overview.containers} />
      {overview.other.length > 0 && (
        <Section
          title={`More facts (${overview.other.length})`}
          collapsed
          description={<Updated facts={overview.other} />}
        >
          <FactList facts={overview.other} />
        </Section>
      )}
    </Stack>
  );
}
