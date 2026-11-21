import {
  Button,
  Cluster,
  Field,
  IconButton,
  NumberInput,
  Select,
  Stack,
  Text,
  TextInput,
} from "../../components";
import { zoneHint } from "./policyText";
import { newCondition, type ConditionForm } from "./ruleModel";
import { zoneLabel } from "./policyZone";
import styles from "./draft.module.css";

type Kind = Exclude<ConditionForm["kind"], "unknown">;

const KINDS: { label: string; value: Kind }[] = [
  { value: "arg_constraint", label: "Argument limit" },
  { value: "time_window", label: "Time window" },
  { value: "requires_fact", label: "Required fact" },
  { value: "max_rate", label: "Rate limit" },
  { value: "max_ttl", label: "Lifetime limit" },
];

const NO_ZONE = "The hub did not say which time zone it uses.";

function Body({
  condition: c,
  error,
  zone,
  patch,
}: {
  condition: ConditionForm;
  error: string | undefined;
  patch: (change: Partial<ConditionForm>) => void;
  zone: string | undefined;
}) {
  switch (c.kind) {
    case "arg_constraint":
      return (
        <div className={styles.fieldRow}>
          <Field label="Argument" error={error}>
            <TextInput mono value={c.arg} onChange={(e) => patch({ arg: e.target.value })} />
          </Field>
          <Field label="Comparison">
            <Select
              value={c.op}
              onChange={(e) => patch({ op: e.target.value as "int_at_least" | "int_at_most" })}
            >
              <option value="int_at_least">is at least</option>
              <option value="int_at_most">is at most</option>
            </Select>
          </Field>
          <Field label="Limit">
            <NumberInput value={c.value} onChange={(e) => patch({ value: e.target.value })} />
          </Field>
        </div>
      );
    case "time_window":
      return (
        <div className={styles.fieldRow}>
          <Field
            label={`Start (${zoneLabel(zone)})`}
            hint={zone ? zoneHint(zone) : undefined}
            error={error}
            locked={zone ? undefined : NO_ZONE}
          >
            <TextInput
              type="time"
              value={c.start}
              onChange={(e) => patch({ start: e.target.value })}
            />
          </Field>
          <Field
            label={`End (${zoneLabel(zone)})`}
            hint={zone ? zoneHint(zone) : undefined}
            locked={zone ? undefined : NO_ZONE}
          >
            <TextInput type="time" value={c.end} onChange={(e) => patch({ end: e.target.value })} />
          </Field>
        </div>
      );
    case "requires_fact":
      return (
        <div className={styles.fieldRow}>
          <Field label="Fact" error={error}>
            <TextInput mono value={c.fact} onChange={(e) => patch({ fact: e.target.value })} />
          </Field>
          <Field label="Must be">
            <Select
              value={c.expected}
              onChange={(e) => patch({ expected: e.target.value as "true" | "false" })}
            >
              <option value="true">is true</option>
              <option value="false">is false</option>
            </Select>
          </Field>
        </div>
      );
    case "max_rate":
      return (
        <Field label="Most per hour" error={error}>
          <NumberInput value={c.max} onChange={(e) => patch({ max: e.target.value })} />
        </Field>
      );
    case "max_ttl":
      return (
        <Field label="Longest lifetime (seconds)" error={error}>
          <NumberInput value={c.seconds} onChange={(e) => patch({ seconds: e.target.value })} />
        </Field>
      );
    default:
      return <Text muted>A condition this page does not know</Text>;
  }
}

/**
 * The conditions of a rule as editable rows: kind, its values, and Remove. A condition this
 * page does not know is shown read-only and kept as it is.
 * @param conditions The rows.
 * @param problems Short sentences by condition key from `validateRule`.
 * @param zone The hub's time zone; without one the time inputs are locked with the reason.
 * @param onChange Receives the whole new list.
 */
export function ConditionFields({
  conditions,
  problems,
  zone,
  onChange,
}: {
  conditions: ConditionForm[];
  onChange: (next: ConditionForm[]) => void;
  problems: Record<string, string>;
  zone: string | undefined;
}) {
  const change = (key: string, next: (c: ConditionForm) => ConditionForm) =>
    onChange(conditions.map((c) => (c.key === key ? next(c) : c)));
  return (
    <Stack gap={3}>
      {conditions.map((c) => (
        <div key={c.key} className={styles.condition}>
          <Cluster justify="between" align="start">
            {c.kind === "unknown" ? (
              <Text strong>Condition</Text>
            ) : (
              <Field label="Condition">
                <Select
                  value={c.kind}
                  onChange={(e) =>
                    change(c.key, () => ({
                      ...newCondition(e.target.value as Kind),
                      key: c.key,
                    }))
                  }
                >
                  {KINDS.map((k) => (
                    <option key={k.value} value={k.value}>
                      {k.label}
                    </option>
                  ))}
                </Select>
              </Field>
            )}
            <IconButton
              icon="trash"
              label="Remove condition"
              onClick={() => onChange(conditions.filter((x) => x.key !== c.key))}
            />
          </Cluster>
          <Body
            condition={c}
            error={problems[c.key]}
            zone={zone}
            patch={(p) => change(c.key, (x) => ({ ...x, ...p }) as ConditionForm)}
          />
        </div>
      ))}
      <div>
        <Button icon="plus" onClick={() => onChange([...conditions, newCondition("max_rate")])}>
          Add condition
        </Button>
      </div>
    </Stack>
  );
}
