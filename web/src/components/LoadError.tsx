import { describeError } from "../design/messages";
import { Button } from "./Button";
import { ErrorDetail } from "./ErrorDetail";
import { InlineMessage } from "./InlineMessage";

/**
 * Shown in place of content that could not load: what failed, why, and Retry. `context` is
 * the verb phrase of what was loading ("load hosts").
 *
 * Use it for every failed query (or let `QueryBoundary` do it). A failed load must never
 * look like an empty list.
 */
export function LoadError({
  error,
  onRetry,
  context,
}: {
  context: string;
  error: unknown;
  /** Usually the query's `refetch`. */
  onRetry?: (() => unknown) | undefined;
}) {
  const described = describeError(error, context);
  return (
    <InlineMessage
      tone="danger"
      title={described.title}
      action={
        onRetry && (
          <Button size="sm" icon="refresh" onClick={() => void onRetry()}>
            Retry
          </Button>
        )
      }
    >
      <ErrorDetail error={described} />
    </InlineMessage>
  );
}
