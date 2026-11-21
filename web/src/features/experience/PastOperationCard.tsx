import { Link } from "react-router";
import type { PastOperation } from "../../api/types";
import { Card, Cluster, RelativeTime, StatusPill, Stack, Tag, Text } from "../../components";
import { operationStatusMeta } from "../../design/tone";
import { Narrative } from "./Narrative";
import styles from "./experience.module.css";

/** Why a past operation resembles the one being looked at: a code and, for an action, its name. */
export interface SimilarReasonLike {
  code: string;
  detail?: string | null | undefined;
}

/** The words for a reason code; an unknown code shows as it came. */
export function reasonLabel(reason: SimilarReasonLike): string {
  switch (reason.code) {
    case "same_check":
      return "Same check";
    case "same_target":
      return "Same target";
    case "same_contract":
      return reason.detail ? `Same action: ${reason.detail}` : "Same action";
    case "similar_words":
      return "Similar words";
    default:
      return reason.code;
  }
}

function hostLabel(operation: PastOperation, id: string): string {
  const host = operation.hosts.find((candidate) => candidate.id === id);
  return host?.name ?? "an unknown host";
}

/**
 * One finished operation as evidence: its goal, outcome and whether the fix was verified,
 * the hosts, what ran and the narrative with the source of each line. Nothing here approves
 * or runs anything. `compact` shortens the narrative; `reasons` and `snippet` come from a
 * similarity list or a search.
 */
export function PastOperationCard({
  operation,
  snippet,
  matchedAnyWord = false,
  reasons = [],
  compact = false,
}: {
  compact?: boolean;
  matchedAnyWord?: boolean;
  operation: PastOperation;
  reasons?: readonly SimilarReasonLike[];
  snippet?: string | null | undefined;
}) {
  return (
    <Card label={operation.goal}>
      <Stack gap={2}>
        <Cluster justify="between">
          <Cluster>
            <StatusPill meta={operationStatusMeta(operation.outcome)} size="sm" />
            <Link to={`/operations/${operation.operation_id}`}>{operation.goal}</Link>
          </Cluster>
          <Text muted size="sm">
            <RelativeTime at={operation.ended_at} />
          </Text>
        </Cluster>
        <Cluster>
          {operation.verified ? (
            <Tag tone="success" hint="A follow-up check confirmed the fix worked.">
              Fix verified
            </Tag>
          ) : (
            <Tag hint="No follow-up check confirmed the fix.">Not verified</Tag>
          )}
          {operation.hosts.map((host) => (
            <Tag key={host.id}>{host.name ?? "a removed host"}</Tag>
          ))}
          {matchedAnyWord && <Tag>Matches some of your words</Tag>}
          {reasons.map((reason) => (
            <Tag key={`${reason.code}:${reason.detail ?? ""}`} tone="info">
              {reasonLabel(reason)}
            </Tag>
          ))}
        </Cluster>
        {snippet && (
          <Text muted size="sm">
            {snippet}
          </Text>
        )}
        {operation.contracts.length > 0 && (
          <ul className={styles.ran}>
            {operation.contracts.map((run, index) => (
              <li key={`${run.contract}:${index}`}>
                <Text size="sm">
                  Ran {run.contract} on {run.target ?? hostLabel(operation, run.host_id)}:{" "}
                  {run.outcome}
                </Text>
              </li>
            ))}
          </ul>
        )}
        <Narrative fields={operation.narrative} compact={compact} />
      </Stack>
    </Card>
  );
}
