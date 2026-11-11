import type { Fact } from "../../api/types";
import { Card, EmptyState, Grid, KeyValueList, RelativeTime, Stack, Text } from "../../components";
import { groupFacts } from "./hostFacts";

function valueOf(fact: Fact) {
  return (
    <Stack gap={1}>
      <span>{fact.unit ? `${fact.value} ${fact.unit}` : fact.value}</span>
      <Text muted size="xs">
        seen <RelativeTime at={fact.observed_at} />
      </Text>
    </Stack>
  );
}

/**
 * What the host's checks measured in the last day, as cards by topic: system, disk, memory
 * and network, with anything else last. A host with no facts says why and what to check.
 */
export function OverviewFacts({ facts, name }: { facts: Fact[]; name: string }) {
  if (facts.length === 0) {
    return (
      <EmptyState icon="info" title="No facts in the last day">
        <p>
          Facts are measurements that checks report, such as free disk space. {name} has sent none,
          so check that its agent is connected and that its checks are on.
        </p>
      </EmptyState>
    );
  }
  return (
    <Grid min="md">
      {groupFacts(facts).map((group) => (
        <Card key={group.id} label={group.title}>
          <Stack gap={3}>
            <Text strong>{group.title}</Text>
            <KeyValueList
              items={group.facts.map((fact) => ({ label: fact.label, value: valueOf(fact) }))}
            />
          </Stack>
        </Card>
      ))}
    </Grid>
  );
}
