import type { Replayed, SimulationItem } from "../../api/types";
import { Section, Stack, Text } from "../../components";
import { DecisionExplanation } from "./decision/DecisionExplanation";
import { decisionOf, type DecisionView } from "./decisionModel";
import styles from "./policy.module.css";

/** Which decisions of a step a simulation covers. */
export type Side = "enforced" | "shadow";

/** The sides a selection covers. */
export function sidesOf(selection: "enforced" | "shadow" | "both"): Side[] {
  return selection === "both" ? ["enforced", "shadow"] : [selection];
}

/** The outcome word of a stored decision, `error` for an evaluation error, or null. */
export function outcomeOf(view: DecisionView | null): string | null {
  if (view === null) return null;
  return view.kind === "error" ? "error" : view.doc.outcome;
}

const SIDE_TITLE: Record<Side, string> = { enforced: "Active policy", shadow: "Shadow rules" };

/** The title that names a side. */
export const sideTitle = (side: Side): string => SIDE_TITLE[side];

function Pair({ replayed, zone }: { replayed: Replayed; zone: string | undefined }) {
  return (
    <div className={styles.compare}>
      <DecisionExplanation
        title="Recorded"
        decision={decisionOf(replayed.baseline)}
        zone={zone}
        emptyText="No rule matched."
      />
      <DecisionExplanation
        title="Simulated"
        decision={decisionOf(replayed.simulated)}
        zone={zone}
        emptyText="No rule matched."
      />
    </div>
  );
}

function Recorded({ replayed, zone }: { replayed: Replayed; zone: string | undefined }) {
  return (
    <Stack gap={3}>
      <Text block muted>
        This decision could not be replayed, so there is no simulated result to compare. The
        recorded decision is shown as it was made.
      </Text>
      <DecisionExplanation
        title="Recorded"
        decision={decisionOf(replayed.baseline)}
        zone={zone}
        emptyText="No rule matched."
      />
    </Stack>
  );
}

/**
 * One simulated step with what was recorded beside what the rules would say, for each side
 * the simulation covered. A decision that could not be replayed shows what was recorded and says why there is no comparison.
 * @param item The step from the simulation.
 * @param sides The sides to show.
 * @param zone The hub's time zone, for time windows.
 */
export function DecisionCompare({
  item,
  sides,
  zone,
}: {
  item: SimulationItem;
  sides: readonly Side[];
  zone: string | undefined;
}) {
  return (
    <Stack gap={5}>
      {sides.map((side) => {
        const replayed = item[side];
        return (
          <Section key={side} title={sideTitle(side)}>
            {!replayed ? (
              <Text block muted>
                This side was not simulated, so there is nothing to compare.
              </Text>
            ) : replayed.comparison === "not_replayable" ? (
              replayed.baseline ? (
                <Recorded replayed={replayed} zone={zone} />
              ) : (
                <Text block muted>
                  This decision could not be replayed, so there is no simulated result to compare,
                  and no decision was recorded for it.
                </Text>
              )
            ) : (
              <Pair replayed={replayed} zone={zone} />
            )}
          </Section>
        );
      })}
    </Stack>
  );
}
