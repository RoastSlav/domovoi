import { Stack } from "../../components";
import { ResolutionSummary } from "./ResolutionSummary";
import { SimilarOperations } from "./SimilarOperations";

/** The statuses of an operation that has ended, and so has a resolution summary. */
const FINISHED = ["succeeded", "failed", "escalated", "cancelled"];

/**
 * What the operation page adds after its details: the resolution summary once the operation
 * has finished (an unknown status is not finished) and similar past operations.
 */
export function OperationExperience({
  operationId,
  status,
}: {
  operationId: string;
  status: string;
}) {
  return (
    <Stack gap={5}>
      {FINISHED.includes(status) && <ResolutionSummary operationId={operationId} />}
      <SimilarOperations source={{ operation: operationId }} />
    </Stack>
  );
}
