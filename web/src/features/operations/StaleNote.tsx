import { Button, InlineMessage, RelativeTime } from "../../components";

/** Says a refresh failed while the page keeps showing what it already loaded. */
export function StaleNote({
  updatedAt,
  onRetry,
}: {
  /** When the data on screen was fetched, in ms since the epoch. */
  updatedAt: number;
  onRetry: () => void;
}) {
  return (
    <InlineMessage
      tone="warning"
      title="Could not refresh this page"
      action={
        <Button size="sm" icon="refresh" onClick={onRetry}>
          Retry
        </Button>
      }
    >
      Showing what loaded <RelativeTime at={new Date(updatedAt).toISOString()} />.
    </InlineMessage>
  );
}
