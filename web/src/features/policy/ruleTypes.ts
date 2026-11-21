/** The outcome a rule form can choose. */
export type FormOutcome = "deny" | "require_approval" | "allow";

/** One condition as the editor holds it: every number is still text. */
export type ConditionForm = { key: string } & (
  | { kind: "arg_constraint"; arg: string; op: "int_at_least" | "int_at_most"; value: string }
  | { kind: "time_window"; start: string; end: string }
  | { kind: "requires_fact"; fact: string; expected: "true" | "false" }
  | { kind: "max_rate"; max: string }
  | { kind: "max_ttl"; seconds: string }
  | { kind: "unknown"; raw: unknown }
);

/** A rule as the editor holds it, ready for text inputs. */
export interface RuleForm {
  id: string;
  enabled: boolean;
  shadow: boolean;
  locked: boolean;
  outcome: FormOutcome;
  /** The host id; empty means every host. */
  hostId: string;
  capKind: "any" | "effect" | "category" | "contract";
  capValue: string;
  targetsKind: "any" | "exact";
  /** One target name per line. */
  targetNames: string;
  conditions: ConditionForm[];
  note: string;
}

/** Field hints for a rule form; absent keys are fine. */
export interface Problems {
  id?: string;
  capValue?: string;
  targetNames?: string;
  note?: string;
  /** Hints keyed by `ConditionForm.key`. */
  conditions: Record<string, string>;
}
