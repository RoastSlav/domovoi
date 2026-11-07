import { useState, type FormEvent } from "react";
import { useHosts } from "../../api/hooks/hosts";
import { useCreateAckRule } from "../../api/hooks/rules";
import { ApiFailure, type AckRuleInput, type Predicate, type PredicateOp } from "../../api/types";
import { Button } from "../../components/Button";
import styles from "./rules.module.css";

/** A condition's operator and the words shown for it. */
export const OPS: [PredicateOp, string][] = [
  ["equals", "equals"],
  ["contains", "contains"],
  ["at_most", "is at most"],
  ["at_least", "is at least"],
  ["in_network", "is in network"],
];

/** How long a rule lives, in minutes, with the words in the add form; "" means never. */
export const EXPIRIES: [string, string][] = [
  ["60", "1 hour"],
  ["1440", "1 day"],
  ["10080", "1 week"],
  ["43200", "30 days"],
  ["", "never"],
];

/** The add form starts at 1 hour. */
const DEFAULT_EXPIRY = "60";

/** Most conditions a rule can have, matching the hub's own limit. */
export const MAX_CONDITIONS = 10;

/** What the add form holds. */
export interface AddRuleFields {
  check: string;
  host: string;
  target: string;
  note: string;
  expiry: string;
  conditions: Predicate[];
}

/** The words the operator shows, for example "is at most". */
export function opLabel(op: PredicateOp): string {
  return OPS.find(([value]) => value === op)?.[1] ?? op;
}

/** The add form as a request body, or the words saying what to fix. */
export function formBody(fields: AddRuleFields): AckRuleInput | string {
  const check = fields.check.trim();
  const note = fields.note.trim();
  if (check === "") {
    return "Type the check key the condition matches, for example security.ssh_login.";
  }
  if (note === "") {
    return "Type why the condition is acceptable, in the note.";
  }
  if (fields.conditions.length > MAX_CONDITIONS) {
    return `Remove a condition: a rule has at most ${MAX_CONDITIONS}.`;
  }
  const conditions: Predicate[] = [];
  for (const [index, condition] of fields.conditions.entries()) {
    const key = condition.key.trim();
    const value = condition.value.trim();
    if (key === "") {
      return `Condition ${index + 1} has no key, for example disk_name.`;
    }
    if (value === "") {
      return `Condition ${index + 1} has no value.`;
    }
    if (
      (condition.op === "at_most" || condition.op === "at_least") &&
      !Number.isFinite(Number(value))
    ) {
      return `Condition ${index + 1} compares a number, so its value is a number.`;
    }
    conditions.push({ key, op: condition.op, value });
  }
  return {
    check,
    host: fields.host === "" ? null : fields.host,
    target_pattern: fields.target.trim() === "" ? null : fields.target.trim(),
    note,
    expires_in_minutes: fields.expiry === "" ? null : Number(fields.expiry),
    predicates: conditions,
  };
}

/** The form that adds a known condition. */
export function AddRuleForm() {
  const create = useCreateAckRule();
  const hosts = useHosts();
  const [fields, setFields] = useState<AddRuleFields>({
    check: "",
    host: "",
    target: "",
    note: "",
    expiry: DEFAULT_EXPIRY,
    conditions: [],
  });
  const [problem, setProblem] = useState<string>();

  const change = (patch: Partial<AddRuleFields>) => setFields({ ...fields, ...patch });

  const setCondition = (index: number, patch: Partial<Predicate>) =>
    setFields({
      ...fields,
      conditions: fields.conditions.map((condition, i) =>
        i === index ? { ...condition, ...patch } : condition,
      ),
    });

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const body = formBody(fields);
    if (typeof body === "string") {
      setProblem(body);
      return;
    }
    setProblem(undefined);
    create.mutate(body, {
      onSuccess: () =>
        setFields({
          check: "",
          host: "",
          target: "",
          note: "",
          expiry: DEFAULT_EXPIRY,
          conditions: [],
        }),
      onError: (error) =>
        setProblem(
          error instanceof ApiFailure ? error.message : "Could not add the known condition.",
        ),
    });
  };

  return (
    <form className={styles.form} onSubmit={onSubmit} aria-label="Add a known condition">
      <h2>Add a known condition</h2>
      <div className={styles.grid}>
        <label className={styles.field}>
          Check key
          <input
            value={fields.check}
            onChange={(e) => change({ check: e.target.value })}
            placeholder="security.ssh_login"
          />
        </label>
        <label className={styles.field}>
          Host
          <select value={fields.host} onChange={(e) => change({ host: e.target.value })}>
            <option value="">Any host</option>
            {(hosts.data ?? []).map((host) => (
              <option key={host.id} value={host.name}>
                {host.name}
              </option>
            ))}
          </select>
        </label>
        <label className={styles.field}>
          Target pattern
          <input
            value={fields.target}
            onChange={(e) => change({ target: e.target.value })}
            placeholder="/var/log/*"
          />
        </label>
        <label className={styles.field}>
          Expires in
          <select value={fields.expiry} onChange={(e) => change({ expiry: e.target.value })}>
            {EXPIRIES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
      </div>
      <label className={styles.field}>
        Note
        <input
          value={fields.note}
          onChange={(e) => change({ note: e.target.value })}
          placeholder="known backup window"
        />
      </label>
      <div className={styles.conditions}>
        <span>Conditions</span>
        {fields.conditions.map((condition, index) => (
          <div className={styles.row} key={index}>
            <input
              aria-label={`Condition ${index + 1} key`}
              value={condition.key}
              onChange={(e) => setCondition(index, { key: e.target.value })}
              placeholder="disk_name"
            />
            <select
              aria-label={`Condition ${index + 1} operator`}
              value={condition.op}
              onChange={(e) => setCondition(index, { op: e.target.value as PredicateOp })}
            >
              {OPS.map(([op, label]) => (
                <option key={op} value={op}>
                  {label}
                </option>
              ))}
            </select>
            <input
              aria-label={`Condition ${index + 1} value`}
              value={condition.value}
              onChange={(e) => setCondition(index, { value: e.target.value })}
            />
            <Button
              aria-label={`Remove condition ${index + 1}`}
              onClick={() =>
                change({
                  conditions: fields.conditions.filter((_, i) => i !== index),
                })
              }
            >
              Remove
            </Button>
          </div>
        ))}
        <div className={styles.actions}>
          <Button
            onClick={() =>
              change({
                conditions: [...fields.conditions, { key: "", op: "equals", value: "" }],
              })
            }
            disabled={fields.conditions.length >= MAX_CONDITIONS}
          >
            Add condition
          </Button>
          <Button type="submit" variant="primary" disabled={create.isPending}>
            Add
          </Button>
        </div>
      </div>
      {problem && (
        <p className={styles.error} role="alert">
          {problem}
        </p>
      )}
    </form>
  );
}
