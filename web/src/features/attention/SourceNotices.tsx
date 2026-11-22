import { Button, InlineMessage, Text } from "../../components";
import type { AttentionState } from "./useAttentionSources";

/** "a", "a and b", "a, b and c". */
function joined(names: readonly string[]): string {
  return names.length < 2
    ? (names[0] ?? "")
    : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** Names the sources that failed, with one Retry, so a failure never reads as "nothing needs you". */
export function SourceFailures({ failed }: { failed: AttentionState["failed"] }) {
  if (failed.length === 0) {
    return null;
  }
  return (
    <InlineMessage
      tone="warning"
      title={`Could not load ${joined(failed.map((source) => source.name))}`}
      action={
        <Button
          variant="secondary"
          onClick={() => {
            for (const source of failed) {
              source.retry();
            }
          }}
        >
          Retry
        </Button>
      }
    >
      What did load is listed below.
    </InlineMessage>
  );
}

/** Says that a source has more items than its first page. */
export function TruncatedNote({ truncated }: { truncated: AttentionState["truncated"] }) {
  if (truncated.length === 0) {
    return null;
  }
  return (
    <Text muted block>
      More items exist than are listed here. Open Incidents or Operations for the rest.
    </Text>
  );
}
