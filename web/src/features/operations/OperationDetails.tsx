import { Link } from "react-router";
import type { OperationTrace } from "../../api/operationTrace";
import { DetailsScope, KeyValueList, RelativeTime, Stack, Text, Visible } from "../../components";
import { BUDGET_KINDS, budgetLabel } from "../../design/operationText";
import { formatBudget, limitFor } from "./budget";

/**
 * The operation's facts in three blocks: what it may act on (always shown), its incidents and
 * budgets, and its metadata (kind and times), which smaller detail levels fold away.
 */
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
      <Visible category="P0" id="operation.scope">
        <KeyValueList
          items={[
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
      </Visible>
      <Visible category="P1" id="operation.incidents">
        <KeyValueList
          items={[
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
          ]}
        />
      </Visible>
      <Visible category="P1" id="operation.budgets">
        <Stack gap={4}>
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
      </Visible>
      <DetailsScope>
        <Visible category="P3" id="operation.metadata" label="Kind and times" count={3}>
          <KeyValueList
            items={[
              { label: "Kind", value: operation.kind },
              {
                label: "Started",
                value: operation.started_at ? <RelativeTime at={operation.started_at} /> : "–",
              },
              {
                label: "Ended",
                value: operation.ended_at ? <RelativeTime at={operation.ended_at} /> : "–",
              },
            ]}
          />
        </Visible>
      </DetailsScope>
    </Stack>
  );
}
