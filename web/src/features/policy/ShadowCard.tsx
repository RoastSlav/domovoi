import type { PolicyOverview } from "../../api/types";
import { Grid, InlineMessage, RelativeTime, StatTile, Stack, Text } from "../../components";
import { shadowStatusMeta } from "../../design/tone";

/**
 * How the shadow recorder is doing: whether it is on, whether its last pass failed, and what
 * it recorded in the last 24 hours.
 * @param overview The policy overview from the hub.
 */
export function ShadowCard({ overview }: { overview: PolicyOverview }) {
  const { shadow, shadow_enabled: enabled } = overview;
  const day = shadow.last_24h;
  return (
    <Stack gap={3}>
      {!enabled && (
        <InlineMessage tone="info">
          Decisions are not being recorded. Set shadow to true under [policy] in hub.toml and
          restart the hub to start.
        </InlineMessage>
      )}
      {shadow.last_error && (
        <InlineMessage tone="danger" title="The last recording pass failed">
          {shadow.last_error}
        </InlineMessage>
      )}
      {enabled && (
        <>
          <Grid min="xs" gap={3}>
            <StatTile label="Recorded" value={day.total} hint="in the last 24 hours" />
            <StatTile label="Degraded" value={day.degraded} />
            <StatTile label="Differing" value={day.differing} />
            <StatTile
              label={shadowStatusMeta("unevaluable").label}
              value={day.by_status.unevaluable ?? 0}
            />
          </Grid>
          <Text muted size="sm">
            {shadow.last_sweep_at ? (
              <>
                Last pass <RelativeTime at={shadow.last_sweep_at} />
              </>
            ) : (
              "No pass yet"
            )}
          </Text>
        </>
      )}
    </Stack>
  );
}
