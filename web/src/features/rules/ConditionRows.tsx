import type { Predicate, PredicateOp } from "../../api/types";
import {
  Button,
  Card,
  Cluster,
  Field,
  Grid,
  IconButton,
  NumberInput,
  Select,
  Stack,
  Text,
  TextInput,
} from "../../components";
import { OPS } from "./phrase";
import { MAX_CONDITIONS, type ConditionProblems } from "./ruleForm";

/** What each kind of value expects, shown under its input. */
function valueHint(op: PredicateOp): string | undefined {
  if (op === "in_network") {
    return "An address or a range in CIDR form, for example 192.168.1.0/24.";
  }
  if (op === "at_most" || op === "at_least") {
    return "A number, for example 90.";
  }
  return undefined;
}

/** A label with the row's number kept for assistive technology: "Key of condition 2". */
function rowLabel(name: string, number: number) {
  return (
    <>
      {`${name} `}
      <span className="visually-hidden">{`of condition ${number}`}</span>
    </>
  );
}

/** The value input that fits the operator: a number, an address, or text. */
function ValueInput({
  condition,
  onChange,
}: {
  condition: Predicate;
  onChange: (value: string) => void;
}) {
  if (condition.op === "at_most" || condition.op === "at_least") {
    return (
      <NumberInput
        value={condition.value}
        step="any"
        onChange={(event) => onChange(event.target.value)}
      />
    );
  }
  return (
    <TextInput
      mono={condition.op === "in_network"}
      value={condition.value}
      placeholder={condition.op === "in_network" ? "192.168.1.0/24" : undefined}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

function ConditionRow({
  condition,
  number,
  problems,
  onChange,
  onRemove,
}: {
  condition: Predicate;
  number: number;
  onChange: (patch: Partial<Predicate>) => void;
  onRemove: () => void;
  problems: ConditionProblems | undefined;
}) {
  return (
    <Card label={`Condition ${number}`}>
      <Stack gap={3}>
        <Cluster justify="between">
          <Text strong>{`Condition ${number}`}</Text>
          <IconButton
            icon="trash"
            size="sm"
            label={`Remove condition ${number}`}
            onClick={onRemove}
          />
        </Cluster>
        <Grid min="sm" gap={3}>
          <Field
            label={rowLabel("Key", number)}
            hint="What the incident reports, for example disk_name."
            error={problems?.key}
          >
            <TextInput
              mono
              value={condition.key}
              placeholder="disk_name"
              onChange={(event) => onChange({ key: event.target.value })}
            />
          </Field>
          <Field label={rowLabel("Compare", number)} error={problems?.op}>
            <Select
              value={condition.op}
              onChange={(event) => onChange({ op: event.target.value as PredicateOp })}
            >
              {OPS.map(([op, label]) => (
                <option key={op} value={op}>
                  {label}
                </option>
              ))}
            </Select>
          </Field>
          <Field
            label={rowLabel("Value", number)}
            hint={valueHint(condition.op)}
            error={problems?.value}
          >
            <ValueInput condition={condition} onChange={(value) => onChange({ value })} />
          </Field>
        </Grid>
      </Stack>
    </Card>
  );
}

/**
 * The condition rows of the add form, all of which must hold for a rule to match, with
 * add and remove. Each row's problems, from the form or from the hub, show under its inputs.
 */
export function ConditionRows({
  conditions,
  problems,
  onChange,
}: {
  conditions: readonly Predicate[];
  onChange: (conditions: Predicate[]) => void;
  problems: Record<number, ConditionProblems>;
}) {
  const edit = (index: number, patch: Partial<Predicate>) =>
    onChange(conditions.map((row, at) => (at === index ? { ...row, ...patch } : row)));
  return (
    <Stack gap={3}>
      <Text muted size="sm" block>
        Conditions narrow the rule to incidents whose evidence fits. All of them must hold. With
        none, the rule matches every incident of the check.
      </Text>
      {conditions.map((condition, index) => (
        <ConditionRow
          // Rows have no identity of their own; they are edited and removed by position.
          key={index}
          condition={condition}
          number={index + 1}
          problems={problems[index]}
          onChange={(patch) => edit(index, patch)}
          onRemove={() => onChange(conditions.filter((_, at) => at !== index))}
        />
      ))}
      <Cluster>
        <Button
          icon="plus"
          disabledReason={
            conditions.length >= MAX_CONDITIONS
              ? `A rule has at most ${MAX_CONDITIONS} conditions.`
              : undefined
          }
          onClick={() => onChange([...conditions, { key: "", op: "equals", value: "" }])}
        >
          Add condition
        </Button>
      </Cluster>
    </Stack>
  );
}
