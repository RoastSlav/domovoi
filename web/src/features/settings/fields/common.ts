import type { FieldSpec } from "../form";

/** What every field renderer is given on top of its value. */
export interface Common {
  spec: FieldSpec;
  /** Help text. */
  hint?: string | undefined;
  /** Problems with the value, from the hub or the input. */
  errors?: string[] | undefined;
  /** When set, the field is read-only and this says why. */
  locked?: string | undefined;
  /** Where the value comes from: `ui`, `file`, `env:<VAR>` or `default`. Hidden when absent. */
  source?: string | undefined;
}

/** The errors of a field as one string for the kit `Field`. */
export function errorText(errors: string[] | undefined): string | undefined {
  return errors && errors.length > 0 ? errors.join(" ") : undefined;
}
