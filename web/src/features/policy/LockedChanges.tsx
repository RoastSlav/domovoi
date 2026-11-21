import type { LockChange } from "../../api/types";
import { Card, Stack, StatusPill, Text } from "../../components";
import { lockChangeMeta } from "../../design/tone";
import { LOCK_HELP } from "./policyText";

/**
 * The locked rules this draft adds, changes, unlocks or removes, with why they matter.
 * @param changes What the review found; shown only when there is something to show.
 */
export function LockedChanges({ changes }: { changes: readonly LockChange[] }) {
  return (
    <Card tone="danger">
      <Stack gap={3}>
        <Text block>{LOCK_HELP}</Text>
        {changes.map((item) => (
          <Stack key={`${item.rule}:${item.change}`} gap={1}>
            <StatusPill meta={lockChangeMeta(item.change)} />
            <Text mono>{item.rule}</Text>
          </Stack>
        ))}
      </Stack>
    </Card>
  );
}
