import type { ConditionForm, Problems, RuleForm } from "./ruleTypes";

const NAME = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,63}$/;
const DOTTED = /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/;
const ARG = /^[a-z][a-z0-9_]{0,63}$/;
const FACT = /^[a-z][a-z0-9_.]{0,63}$/;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;
const INT = /^-?\d+$/;
const NATURAL = /^\d+$/;
const CONTROL = /[\u0000-\u001f\u007f]/; // eslint-disable-line no-control-regex
const WHOLE = "Enter a whole number.";

/**
 * Splits the one-per-line target text into names.
 * @param text One target name per line.
 * @returns Trimmed, de-duplicated, non-blank names in the order given.
 */
export function splitTargets(text: string): string[] {
  const seen = new Set<string>();
  for (const line of text.split(/\r?\n/)) {
    const name = line.trim();
    if (name !== "") seen.add(name);
  }
  return [...seen];
}

function conditionProblem(c: ConditionForm): string | undefined {
  switch (c.kind) {
    case "arg_constraint":
      if (!ARG.test(c.arg.trim()))
        return "Use lowercase letters, digits and _, starting with a letter.";
      return INT.test(c.value.trim()) ? undefined : WHOLE;
    case "time_window":
      if (!TIME.test(c.start.trim()) || !TIME.test(c.end.trim())) return "Enter times as HH:MM.";
      return c.start.trim() === c.end.trim() ? "Start and end must differ." : undefined;
    case "requires_fact":
      return FACT.test(c.fact.trim())
        ? undefined
        : "Use lowercase letters, digits, _ and . for the fact.";
    case "max_rate":
      return NATURAL.test(c.max.trim()) ? undefined : WHOLE;
    case "max_ttl":
      return NATURAL.test(c.seconds.trim()) ? undefined : WHOLE;
    default:
      return undefined;
  }
}

function dotted(value: string, max: number, example: string): string | undefined {
  const v = value.trim();
  return v.length <= max && DOTTED.test(v) ? undefined : `Use a dotted name such as ${example}.`;
}

function capabilityProblem(form: RuleForm): string | undefined {
  switch (form.capKind) {
    case "effect":
      return form.capValue.trim() === "" ? "Choose an effect." : undefined;
    case "category":
      return dotted(form.capValue, 64, "container.lifecycle");
    case "contract":
      return dotted(form.capValue, 80, "container.restart or skill.backup_db");
    default:
      return undefined;
  }
}

function targetsProblem(form: RuleForm): string | undefined {
  if (form.targetsKind !== "exact") return undefined;
  const names = splitTargets(form.targetNames);
  if (names.length === 0) return "Name at least one target, or choose Any target.";
  const bad = names.some((n) => n.length > 128 || CONTROL.test(n));
  return bad ? "Each target name is 1 to 128 characters with no control characters." : undefined;
}

/**
 * Checks a rule form field by field.
 * @param form The form to check.
 * @param otherIds The ids of the other rules in the draft.
 * @returns Short sentences by field; see `isValid` for "nothing to fix".
 */
export function validateRule(form: RuleForm, otherIds: readonly string[] = []): Problems {
  const problems: Problems = { conditions: {} };
  const id = form.id.trim();
  if (id === "") problems.id = "Give the rule a name.";
  else if (!NAME.test(id))
    problems.id =
      "Use letters, digits and . _ : -, starting with a letter or digit, up to 64 characters.";
  else if (otherIds.includes(id)) problems.id = "Another rule already has this name.";
  const cap = capabilityProblem(form);
  if (cap) problems.capValue = cap;
  const targets = targetsProblem(form);
  if (targets) problems.targetNames = targets;
  if (form.note.length > 500) problems.note = "At most 500 characters.";
  for (const c of form.conditions) {
    const p = conditionProblem(c);
    if (p) problems.conditions[c.key] = p;
  }
  return problems;
}

/**
 * Whether `validateRule` found nothing to fix.
 * @param problems The result of `validateRule`.
 */
export function isValid(problems: Problems): boolean {
  return (
    !problems.id &&
    !problems.capValue &&
    !problems.targetNames &&
    !problems.note &&
    Object.keys(problems.conditions).length === 0
  );
}
