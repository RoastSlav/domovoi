import { useMemo } from "react";
import { Link } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import type { SimulateInput } from "../../api/hooks/policyCommands";
import type { PolicySimulation, SimulationItem } from "../../api/types";
import {
  Card,
  Cluster,
  DataTable,
  EmptyState,
  InlineMessage,
  Section,
  Stack,
  StatusPill,
  Tag,
  Text,
  Visible,
  type Column,
} from "../../components";
import { comparisonMeta } from "../../design/tone";
import { DecisionCompare, outcomeOf, sideTitle, sidesOf, type Side } from "./DecisionCompare";
import { decisionOf } from "./decisionModel";
import { PolicyNotes } from "./PolicyNotes";
import { useTimeWindowZone } from "./policyZone";
import { OutcomePill } from "./RuleMarks";
import { SimulationSummary } from "./SimulationSummary";

const CURRENT_WORDS = "Current inputs. This is a what-if, not a reproduction of what happened.";
const RECORDED_WORDS =
  "Recorded inputs. Where an input was not available when the decision was made, the result shows the gap.";
const NO_RECORDING =
  "Decisions are not being recorded. Set shadow to true under [policy] in hub.toml and restart the hub.";

function ranWords(request: SimulateInput): string {
  const rules = request.rules.source === "draft" ? "the draft" : `version ${request.rules.number}`;
  const sides = sidesOf(request.selection).map(sideTitle).join(" and ");
  return `Ran ${rules} against ${sides}.`;
}

function Basis({ data, request }: { data: PolicySimulation; request: SimulateInput }) {
  return (
    <Visible category="P0" id="simulator.basis">
      <Card label="What was simulated">
        <Stack gap={2}>
          <Text block strong>
            {data.label}
          </Text>
          <Text block>{ranWords(request)}</Text>
          <Text block>{request.inputs === "current" ? CURRENT_WORDS : RECORDED_WORDS}</Text>
          <PolicyNotes notes={data.notes} />
          <Text block muted>
            {`${data.summary.not_replayable} could not be replayed`}{" "}
            <Tag hint="A record that was skipped or could not be evaluated cannot be replayed.">
              Why?
            </Tag>
          </Text>
          {data.truncated && (
            <Text block>
              {`Showing the newest ${data.items.length} decisions. Narrow the filter to see others.`}
            </Text>
          )}
        </Stack>
      </Card>
    </Visible>
  );
}

function SideCell({ item, side }: { item: SimulationItem; side: Side }) {
  const replayed = item[side];
  if (!replayed) return <Text muted>Not simulated</Text>;
  const before = outcomeOf(decisionOf(replayed.baseline));
  const after = outcomeOf(decisionOf(replayed.simulated));
  return (
    <Cluster gap={2}>
      {before && <OutcomePill outcome={before} />}
      {before && after && <span aria-hidden="true">→</span>}
      {after && <OutcomePill outcome={after} />}
      <StatusPill size="sm" meta={comparisonMeta(replayed.comparison)} />
    </Cluster>
  );
}

/**
 * What a simulation found: how it was run, the counts, a table of steps with the recorded and
 * simulated outcome side by side, and the explanation of the selected step.
 * @param data The hub's answer.
 * @param request What the run was asked, as sent; the page describes the run from it.
 * @param item The execution id of the selected step.
 * @param onItem Selects a step.
 * @param recording Whether the hub records decisions at all.
 */
export function SimulatorResults({
  data,
  request,
  item,
  onItem,
  recording,
}: {
  data: PolicySimulation;
  item: string | null;
  onItem: (execution: string) => void;
  recording: boolean;
  request: SimulateInput;
}) {
  const hostNames = useHostNames();
  const zone = useTimeWindowZone();
  const sides = useMemo(() => sidesOf(request.selection), [request.selection]);
  const columns = useMemo<Column<SimulationItem>[]>(
    () => [
      {
        key: "step",
        label: "Step",
        cell: (r) => (
          <Link to={`/policy/decisions/${r.execution_id}`}>
            <Text mono>{r.contract}</Text>
          </Link>
        ),
      },
      { key: "host", label: "Host", cell: (r) => hostNames.get(r.host_id) ?? "Unknown host" },
      ...sides.map((side) => ({
        key: side,
        label: sideTitle(side),
        cell: (r: SimulationItem) => <SideCell item={r} side={side} />,
      })),
      {
        key: "degraded",
        label: "Inputs",
        cell: (r) => (r.degraded ? <Tag tone="warning">Degraded</Tag> : null),
      },
    ],
    [hostNames, sides],
  );
  const chosen = data.items.find((r) => r.execution_id === item);
  return (
    <Stack gap={5}>
      <Basis data={data} request={request} />
      {data.summary.newly_allowed > 0 && (
        <Visible category="P0" id="simulator.widening">
          <InlineMessage tone="warning">
            {`${data.summary.newly_allowed} decisions would become Allow automatically`}
          </InlineMessage>
        </Visible>
      )}
      <Visible category="P1" id="simulator.summary">
        <SimulationSummary summary={data.summary} />
      </Visible>
      <Visible category="P1" id="simulator.results">
        <DataTable
          caption="Simulated decisions"
          columns={columns}
          rows={data.items}
          rowKey={(r) => r.execution_id}
          onRowClick={(r) => onItem(r.execution_id)}
          empty={
            <EmptyState icon="info" title="No recorded decisions match">
              <p>
                {recording
                  ? "Widen the filter, or run again once more steps have been recorded."
                  : NO_RECORDING}
              </p>
            </EmptyState>
          }
        />
      </Visible>
      {chosen && (
        <Visible category="P1" id="simulator.detail">
          <Section title={`Step ${chosen.contract}`}>
            <DecisionCompare item={chosen} sides={sides} zone={zone} />
          </Section>
        </Visible>
      )}
    </Stack>
  );
}
