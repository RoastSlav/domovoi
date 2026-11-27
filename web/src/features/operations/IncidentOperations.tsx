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
import { QuestionCard } from "./QuestionCard";

/** A clock time such as "14:05" for an RFC 3339 instant. */
function clock(at: string): string {
  const date = new Date(at);
  return Number.isNaN(date.getTime())
    ? at
    : date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

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
                <Stack gap={2}>
                  <InlineMessage tone="warning" title="Triage is waiting for your answer">
                    <p>
                      It asked a question it could not answer from the host.{" "}
                      {operation.question && (
                        <>
                          If you do not answer, it carries on without you at{" "}
                          <strong>{clock(operation.question.expires_at)}</strong>, and the report
                          says it asked and got no answer.{" "}
                        </>
                      )}
                      <Link to={`/operations/${operation.id}`}>Open the operation</Link>.
                    </p>
                  </InlineMessage>
                  {operation.question && (
                    <QuestionCard
                      operationId={operation.id}
                      step={{ id: operation.question.step, summary: operation.question.text }}
                      paused={false}
                    />
                  )}
                </Stack>
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
