import { Link } from "react-router";
import type { OperationSummary } from "../../api/types";
import { Cluster, RelativeTime, Section, Stack, StatusPill, Text } from "../../components";
import { operationStatusMeta } from "../../design/tone";

/** The investigations of one incident, newest first; nothing at all when there are none. */
export function IncidentOperations({ operations }: { operations: readonly OperationSummary[] }) {
  if (operations.length === 0) {
    return null;
  }
  return (
    <Section title="Investigations">
      <Stack gap={2}>
        {operations.map((operation) => (
          <Cluster key={operation.id} justify="between">
            <Cluster>
              <StatusPill meta={operationStatusMeta(operation.status)} size="sm" />
              <Link to={`/operations/${operation.id}`}>{operation.goal}</Link>
            </Cluster>
            <Text muted size="sm">
              <RelativeTime at={operation.updated_at} />
            </Text>
          </Cluster>
        ))}
      </Stack>
    </Section>
  );
}
