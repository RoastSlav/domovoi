import type { PolicyRule } from "../../api/types";
import type { ConditionForm, FormOutcome, Problems, RuleForm } from "./ruleTypes";
import { isValid, splitTargets, validateRule } from "./ruleValidation";

export type { ConditionForm, FormOutcome, Problems, RuleForm } from "./ruleTypes";
export { isValid, splitTargets, validateRule } from "./ruleValidation";

type Condition = NonNullable<PolicyRule["conditions"]>[number];

let keyCounter = 0;
function nextKey(): string {
  keyCounter += 1;
  return `c${keyCounter}`;
}

/**
 * An empty condition of a kind, ready to edit.
 * @param kind Which condition to start.
 */
export function newCondition(
  kind: "arg_constraint" | "time_window" | "requires_fact" | "max_rate" | "max_ttl",
): ConditionForm {
  const key = nextKey();
  switch (kind) {
    case "arg_constraint":
      return { key, kind, arg: "", op: "int_at_least", value: "" };
    case "time_window":
      return { key, kind, start: "", end: "" };
    case "requires_fact":
      return { key, kind, fact: "", expected: "true" };
    case "max_rate":
      return { key, kind, max: "" };
    default:
      return { key, kind, seconds: "" };
  }
}

function conditionFromDto(c: Condition): ConditionForm {
  const key = nextKey();
  switch (c.kind) {
    case "arg_constraint":
      return c.op === "int_at_least" || c.op === "int_at_most"
        ? { key, kind: "arg_constraint", arg: c.arg, op: c.op, value: String(c.value) }
        : { key, kind: "unknown", raw: c };
    case "time_window":
      return { key, kind: "time_window", start: c.start, end: c.end };
    case "requires_fact":
      return { key, kind: "requires_fact", fact: c.fact, expected: c.expected ? "true" : "false" };
    case "max_rate":
      return { key, kind: "max_rate", max: String(c.max_per_hour) };
    case "max_ttl":
      return { key, kind: "max_ttl", seconds: String(c.max_ttl_secs) };
    default:
      return { key, kind: "unknown", raw: c };
  }
}

function capabilityValue(cap: PolicyRule["capability"]): string {
  switch (cap.kind) {
    case "effect":
      return cap.effect;
    case "category":
      return cap.category;
    case "contract":
      return cap.contract;
    default:
      return "";
  }
}

/**
 * Turns a stored rule into a form.
 * @param rule The rule as the hub stores it.
 */
export function fromDto(rule: PolicyRule): RuleForm {
  const { capability, targets } = rule;
  const known =
    ["deny", "require_approval", "allow"].includes(rule.outcome) &&
    ["all", "host"].includes(rule.host.kind) &&
    ["any", "effect", "category", "contract"].includes(capability.kind) &&
    ["any", "exact"].includes(targets.kind);
  return {
    unsupported: known ? null : rule,
    id: rule.id,
    enabled: rule.enabled,
    shadow: rule.shadow,
    locked: rule.locked,
    outcome: known ? (rule.outcome as FormOutcome) : "require_approval",
    hostId: rule.host.kind === "host" ? rule.host.host_id : "",
    capKind: capability.kind,
    capValue: capabilityValue(capability),
    targetsKind: targets.kind,
    targetNames: targets.kind === "exact" ? targets.names.join("\n") : "",
    conditions: (rule.conditions ?? []).map(conditionFromDto),
    note: rule.note ?? "",
  };
}

function conditionToDto(c: ConditionForm): Condition {
  switch (c.kind) {
    case "arg_constraint":
      return { kind: "arg_constraint", arg: c.arg.trim(), op: c.op, value: Number(c.value.trim()) };
    case "time_window":
      return { kind: "time_window", start: c.start.trim(), end: c.end.trim() };
    case "requires_fact":
      return { kind: "requires_fact", fact: c.fact.trim(), expected: c.expected === "true" };
    case "max_rate":
      return { kind: "max_rate", max_per_hour: Number(c.max.trim()) };
    case "max_ttl":
      return { kind: "max_ttl", max_ttl_secs: Number(c.seconds.trim()) };
    default:
      return c.raw as Condition;
  }
}

function capabilityToDto(form: RuleForm): PolicyRule["capability"] {
  const value = form.capValue.trim();
  switch (form.capKind) {
    case "effect":
      return { kind: "effect", effect: value };
    case "category":
      return { kind: "category", category: value };
    case "contract":
      return { kind: "contract", contract: value };
    default:
      return { kind: "any" };
  }
}

/**
 * Turns a form into the stored shape, after validating it.
 * @param form The form to convert.
 * @param otherIds The ids of the other rules in the draft.
 * @returns The rule, or the problems that stop it.
 */
export function toDto(
  form: RuleForm,
  otherIds: readonly string[] = [],
): { ok: true; rule: PolicyRule } | { ok: false; problems: Problems } {
  if (form.unsupported) return { ok: true, rule: form.unsupported };
  const problems = validateRule(form, otherIds);
  if (!isValid(problems)) return { ok: false, problems };
  const host = form.hostId.trim();
  const note = form.note.trim();
  const rule: PolicyRule = {
    id: form.id.trim(),
    enabled: form.enabled,
    shadow: form.shadow,
    locked: form.locked && form.outcome === "deny",
    outcome: form.outcome,
    host: host === "" ? { kind: "all" } : { kind: "host", host_id: host },
    capability: capabilityToDto(form),
    targets:
      form.targetsKind === "exact"
        ? { kind: "exact", names: splitTargets(form.targetNames) }
        : { kind: "any" },
    conditions: form.conditions.map(conditionToDto),
    ...(note === "" ? {} : { note }),
  };
  return { ok: true, rule };
}

function uniqueId(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n += 1) {
    const candidate = `${base}-${n}`;
    if (!taken.includes(candidate)) return candidate;
  }
}

/**
 * A fresh rule that asks for approval and is only recorded (shadow).
 * @param existingIds The ids already in the draft.
 */
export function newRule(existingIds: readonly string[]): RuleForm {
  return {
    id: uniqueId("new-rule", existingIds),
    enabled: true,
    shadow: true,
    locked: false,
    outcome: "require_approval",
    hostId: "",
    capKind: "any",
    capValue: "",
    targetsKind: "any",
    targetNames: "",
    conditions: [],
    note: "",
    unsupported: null,
  };
}

/**
 * A copy of a rule under a new name; the copy is a shadow rule and never locked.
 * @param rule The rule to copy.
 * @param existingIds The ids already in the draft.
 */
export function duplicateOf(rule: PolicyRule, existingIds: readonly string[]): RuleForm {
  const id = uniqueId(`${rule.id}-copy`, existingIds);
  const form = fromDto(rule);
  return {
    ...form,
    id,
    locked: false,
    shadow: true,
    unsupported: form.unsupported && { ...form.unsupported, id, locked: false, shadow: true },
  };
}

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : 1));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonical(v)}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function normal(rule: PolicyRule): string {
  return canonical({ ...rule, conditions: rule.conditions ?? [], note: rule.note ?? "" });
}

/**
 * Counts the rules that differ between two rule sets.
 * @param before The earlier rules.
 * @param after The later rules.
 * @returns Added plus removed plus changed rules, matched by id.
 */
export function diffCount(before: readonly PolicyRule[], after: readonly PolicyRule[]): number {
  const old = new Map(before.map((r) => [r.id, normal(r)]));
  let count = 0;
  for (const rule of after) {
    const was = old.get(rule.id);
    if (was === undefined || was !== normal(rule)) count += 1;
    old.delete(rule.id);
  }
  return count + old.size;
}
