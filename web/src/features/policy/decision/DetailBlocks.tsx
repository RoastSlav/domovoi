import {
  Cluster,
  InlineMessage,
  KeyValueList,
  Stack,
  StatusPill,
  Tag,
  Text,
} from "../../../components";
import { factStatusMeta } from "../../../design/tone";
import type { DecisionDoc } from "../decisionModel";

/** Every rule that matched, marked Winner, Overridden or Locked. */
export function MatchedBlock({ doc }: { doc: DecisionDoc }) {
  if (doc.matched.length === 0) return null;
  const overridden = new Set(doc.overridden.map((o) => o.broader));
  return (
    <Stack gap={2}>
      {doc.matched.map((name) => (
        <Cluster key={name} gap={2}>
          <Text mono>{name}</Text>
          {doc.winners.includes(name) && <Tag tone="success">Winner</Tag>}
          {overridden.has(name) && <Tag>Overridden</Tag>}
          {doc.locked.includes(name) && <Tag tone="danger">Locked</Tag>}
        </Cluster>
      ))}
    </Stack>
  );
}

/** The safety facts a decision consulted, by whether each was known. */
export function FactsBlock({ doc }: { doc: DecisionDoc }) {
  if (doc.facts.length === 0) return null;
  return (
    <Stack gap={2}>
      {doc.facts.map((f) => (
        <div key={f.fact}>
          <Text mono>{f.fact}</Text> <StatusPill size="sm" meta={factStatusMeta(f.status)} />
        </div>
      ))}
      <Text block muted>
        Only whether each fact was known is recorded, never its value.
      </Text>
    </Stack>
  );
}

/** A warning when the page could not read part of the decision, so it is not taken as complete. */
export function IncompleteBlock({ doc }: { doc: DecisionDoc }) {
  if (!doc.incomplete) return null;
  return (
    <InlineMessage tone="warning" title="Part of this explanation could not be read">
      This page does not know everything the hub recorded here, so do not treat the explanation as
      complete.
    </InlineMessage>
  );
}

/** The short labels of what the page could not read, for people checking the record. */
export function UnreadableBlock({ doc }: { doc: DecisionDoc }) {
  if (doc.unreadable.length === 0) return null;
  return (
    <Stack gap={1}>
      {doc.unreadable.map((label) => (
        <Text key={label} mono>
          {label}
        </Text>
      ))}
    </Stack>
  );
}

/**
 * A stored value as short readable text.
 * @param v Any value from a recorded step or inputs.
 */
export function plain(v: unknown): string {
  if (typeof v === "string") return v;
  try {
    return JSON.stringify(v) ?? String(v);
  } catch {
    return String(v);
  }
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/**
 * The step a decision was made for: its target and arguments, read defensively.
 * @param candidate The stored candidate; any shape is shown, never rejected.
 */
export function StepFacts({ candidate }: { candidate: unknown }) {
  if (!isRecord(candidate)) return <Text mono>{plain(candidate)}</Text>;
  const args = candidate.args;
  const argRows = isRecord(args)
    ? Object.entries(args).map(([k, v]) => `${k} = ${plain(v)}`)
    : args === undefined
      ? []
      : [plain(args)];
  return (
    <KeyValueList
      mono
      items={[
        ...(candidate.target !== undefined
          ? [{ label: "Target", value: plain(candidate.target) }]
          : []),
        {
          label: "Arguments",
          value: argRows.length === 0 ? "none" : argRows.map((r) => <div key={r}>{r}</div>),
        },
      ]}
    />
  );
}

/**
 * The inputs the evaluator was given, as readable key and value rows.
 * @param inputs The stored inputs; unknown shapes show as compact JSON.
 */
export function InputsBlock({ inputs }: { inputs: unknown }) {
  if (isRecord(inputs)) {
    return (
      <KeyValueList
        mono
        items={Object.entries(inputs).map(([k, v]) => ({ label: k, value: plain(v) }))}
      />
    );
  }
  return <Text mono>{plain(inputs)}</Text>;
}
