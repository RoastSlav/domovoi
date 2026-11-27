import { Link } from "react-router";
import type { OperationSummary } from "../../api/types";
import {
  Cluster,
  InlineMessage,
  RelativeTime,
  Section,
  Stack,
  StatusPill,
  Text,
  Visible,
} from "../../components";
import { operationStatusMeta } from "../../design/tone";

/** The investigations of one incident, newest first; nothing at all when there are none. */
export function IncidentOperations({ operations }: { operations: readonly OperationSummary[] }) {
  if (operations.length === 0) {
    return null;
  }
  return (
    <Visible category="P1" id="incident.investigations">
      <Section title="Investigations">
        <Stack gap={2}>
          {operations
            .filter((operation) => operation.status === "waiting_user")
            .map((operation) => (
              <Visible key={operation.id} category="P0" id="incident.question">
                <InlineMessage tone="warning" title="Triage is waiting for your answer">
                  <p>
                    It asked a question it could not answer from the host.{" "}
                    <Link to={`/operations/${operation.id}`}>Read the question and answer it</Link>.
                    If you do not, it carries on by itself when the question expires.
                  </p>
                </InlineMessage>
              </Visible>
            ))}
          {operations.map((operation) => (
            <Cluster key={operation.id} justify="between">
              <Cluster>
                <Visible category="P0" id="incident.investigation-status">
                  <StatusPill meta={operationStatusMeta(operation.status)} size="sm" />
                </Visible>
                <Link to={`/operations/${operation.id}`}>{operation.goal}</Link>
              </Cluster>
              <Text muted size="sm">
                <RelativeTime at={operation.updated_at} />
              </Text>
            </Cluster>
          ))}
        </Stack>
      </Section>
    </Visible>
  );
}
