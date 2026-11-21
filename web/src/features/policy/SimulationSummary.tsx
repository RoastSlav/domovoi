import type { SimulationSummary as Summary } from "../../api/types";
import { Grid, StatTile, Text } from "../../components";

/**
 * The counts of a simulation: how many decisions stayed, changed or could not be replayed,
 * and how many would become Allow, Deny or ask for approval.
 * @param summary The counts from the hub.
 * @param label What was simulated, shown above the tiles.
 */
export function SimulationSummary({ summary, label }: { summary: Summary; label?: string }) {
  return (
    <>
      {label && (
        <Text muted block>
          {label}
        </Text>
      )}
      <Grid min="xs" gap={3}>
        <StatTile label="Evaluated" value={summary.evaluated} />
        <StatTile label="Same" value={summary.same} />
        <StatTile label="Changed" value={summary.changed} />
        <StatTile label="Cannot replay" value={summary.not_replayable} />
        <StatTile
          label="Would become Allow automatically"
          value={summary.newly_allowed}
          tone={summary.newly_allowed > 0 ? "warning" : undefined}
        />
        <StatTile label="Would become Deny" value={summary.newly_denied} />
        <StatTile label="Would ask for approval" value={summary.newly_requires_approval} />
      </Grid>
    </>
  );
}
