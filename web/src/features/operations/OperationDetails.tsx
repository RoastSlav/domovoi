import { Link } from "react-router";
import type { OperationTrace } from "../../api/operationTrace";
import { KeyValueList, RelativeTime, Stack, Text } from "../../components";
import { BUDGET_KINDS, budgetLabel } from "../../design/operationText";
import { formatBudget, limitFor } from "./budget";

/** The operation's facts: kind, incidents, times, what it may act on, and its budgets. */
export function OperationDetails({
  trace,
  hostNames,
}: {
  trace: OperationTrace;
  hostNames: Map<string, string>;
}) {
  const { operation } = trace;
  const pinned = operation.pinned.map(
    (subject) =>
      `${hostNames.get(subject.host) ?? subject.host}: ${subject.check}${subject.target ? ` on ${subject.target}` : ""}`,
  );
  return (
    <Stack gap={4}>
      <KeyValueList
        items={[
          { label: "Kind", value: operation.kind },
          {
            label: "Incidents",
            value:
              operation.incidents.length === 0
                ? "–"
                : operation.incidents.map((id, index) => (
                    <span key={id}>
                      {index > 0 && ", "}
                      <Link to={`/incidents/${id}`}>{id}</Link>
                    </span>
                  )),
          },
          {
            label: "Started",
            value: operation.started_at ? <RelativeTime at={operation.started_at} /> : "–",
          },
          {
            label: "Ended",
            value: operation.ended_at ? <RelativeTime at={operation.ended_at} /> : "–",
          },
          {
            label: "Pinned subjects",
            hint: "The only hosts, checks and targets an action of this operation may name.",
            value:
              pinned.length === 0
                ? "–"
                : pinned.map((line) => (
                    <span key={line}>
                      {line}
                      <br />
                    </span>
                  )),
          },
        ]}
      />
      <Text strong block>
        Budgets
      </Text>
      <KeyValueList
        items={BUDGET_KINDS.map((kind) => {
          const { limit, used } = limitFor(kind, trace);
          return { label: budgetLabel(kind), value: formatBudget(kind, used, limit) };
        })}
      />
    </Stack>
  );
}
