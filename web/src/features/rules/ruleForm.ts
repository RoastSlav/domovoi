import type { AckRuleInput, Predicate } from "../../api/types";
import type { DescribedError } from "../../design/messages";

/** Most conditions a rule can have, matching the hub's own limit. */
export const MAX_CONDITIONS = 10;

/** Longest note the hub accepts. */
export const MAX_NOTE = 500;

/** What the add form holds. */
export interface RuleFields {
  check: string;
  conditions: Predicate[];
  /** Minutes the rule lives, as text; `""` means it never expires. */
  expiry: string;
  host: string;
  note: string;
  target: string;
}

/** The add form starts empty, limited to `host`, for one hour. */
export function emptyFields(host: string): RuleFields {
  return { check: "", host, target: "", note: "", expiry: "60", conditions: [] };
}

/** What is wrong with one condition row, by input. */
export interface ConditionProblems {
  key?: string;
  op?: string;
  value?: string;
}

/** What is wrong with the form, by field; the conditions by row. */
export interface RuleProblems {
  check?: string;
  conditions: Record<number, ConditionProblems>;
  expiry?: string;
  host?: string;
  note?: string;
  target?: string;
}

/** A problems object with nothing wrong. */
export const NO_PROBLEMS: RuleProblems = { conditions: {} };

/** Whether anything is wrong. */
export function hasProblems(problems: RuleProblems): boolean {
  return (
    Boolean(
      problems.check || problems.expiry || problems.host || problems.note || problems.target,
    ) || Object.keys(problems.conditions).length > 0
  );
}

/** An address or a range in CIDR form, loosely: the hub does the exact check. */
const NETWORK = /^[0-9a-fA-F:.]+(\/\d{1,3})?$/;

/** What is wrong with condition `condition`, if anything. */
function conditionProblems(condition: Predicate): ConditionProblems {
  const problems: ConditionProblems = {};
  const value = condition.value.trim();
  if (condition.key.trim() === "") {
    problems.key = "Type the evidence key, for example disk_name.";
  }
  if (value === "") {
    problems.value = "Type the value to compare with.";
  } else if (
    (condition.op === "at_most" || condition.op === "at_least") &&
    !Number.isFinite(Number(value))
  ) {
    problems.value = "Type a number, for example 90.";
  } else if (condition.op === "in_network" && !NETWORK.test(value)) {
    problems.value = "Type an address or a range in CIDR form, for example 192.168.1.0/24.";
  }
  return problems;
}

/** The form as a request body, or what to fix in which field. */
export function buildBody(fields: RuleFields): { body: AckRuleInput } | RuleProblems {
  const problems: RuleProblems = { conditions: {} };
  const note = fields.note.trim();
  if (fields.check.trim() === "") {
    problems.check = "Type the check the condition matches, for example security.ssh_login.";
  }
  if (note === "") {
    problems.note = "Type why the condition is acceptable.";
  } else if (note.length > MAX_NOTE) {
    problems.note = `Keep the note to ${MAX_NOTE} characters or fewer.`;
  }
  fields.conditions.forEach((condition, index) => {
    const found = conditionProblems(condition);
    if (Object.keys(found).length > 0) {
      problems.conditions[index] = found;
    }
  });
  if (hasProblems(problems)) {
    return problems;
  }
  const target = fields.target.trim();
  return {
    body: {
      check: fields.check.trim(),
      host: fields.host === "" ? null : fields.host,
      target_pattern: target === "" ? null : target,
      note,
      expires_in_minutes: fields.expiry === "" ? null : Number(fields.expiry),
      predicates: fields.conditions.map((condition) => ({
        key: condition.key.trim(),
        op: condition.op,
        value: condition.value.trim(),
      })),
    },
  };
}

const ROW_ISSUE = /^predicates(?:\[(\d+)\]|\.(\d+))\.(key|op|value)$/;
const FIELD_ISSUES: Record<string, "check" | "host" | "target" | "note" | "expiry"> = {
  check: "check",
  host: "host",
  target_pattern: "target",
  note: "note",
  expires_in_minutes: "expiry",
};

/**
 * Splits the hub's validation issues into the ones that belong to a field or condition row
 * and the rest, which the dialog lists as they are.
 */
export function sortIssues(error: DescribedError | undefined): {
  problems: RuleProblems;
  rest: NonNullable<DescribedError["issues"]>;
} {
  const problems: RuleProblems = { conditions: {} };
  const rest: NonNullable<DescribedError["issues"]> = [];
  for (const issue of error?.issues ?? []) {
    const row = ROW_ISSUE.exec(issue.path);
    const field = FIELD_ISSUES[issue.path];
    if (row) {
      const index = Number(row[1] ?? row[2]);
      const name = row[3] as keyof ConditionProblems;
      problems.conditions[index] = { ...problems.conditions[index], [name]: issue.message };
    } else if (field) {
      problems[field] = issue.message;
    } else {
      rest.push(issue);
    }
  }
  return { problems, rest };
}

/** `a` with whatever `b` says is wrong added: the form's own problems come first. */
export function mergeProblems(a: RuleProblems, b: RuleProblems): RuleProblems {
  const conditions: Record<number, ConditionProblems> = { ...b.conditions };
  for (const [index, found] of Object.entries(a.conditions)) {
    conditions[Number(index)] = { ...conditions[Number(index)], ...found };
  }
  return { ...b, ...a, conditions };
}
