import type { Changes, SecretChange } from "../../../api/hooks/settings";
import { getIn, setIn } from "./paths";
import type { FieldKind, FieldSpec } from "./types";

/** A setting as the input shows it. */
export function toInput(value: unknown, kind: FieldKind): string | boolean {
  if (kind === "bool") {
    return value === true;
  }
  if (value === undefined || value === null) {
    return "";
  }
  if (kind === "list" || kind === "ids") {
    return Array.isArray(value) ? value.map(String).join(", ") : String(value);
  }
  if (kind === "json") {
    return JSON.stringify(value, null, 2);
  }
  return String(value);
}

/** Why an input cannot be saved, if it cannot. */
export function inputProblem(input: string | boolean, kind: FieldKind): string | undefined {
  if (typeof input !== "string" || input.trim() === "") {
    return undefined;
  }
  if (kind === "number" && !Number.isFinite(Number(input))) {
    return "Enter a number.";
  }
  if (kind === "id" && !/^\d+$/.test(input.trim())) {
    return "Enter a Discord ID using digits only.";
  }
  if (kind === "ids" && !input.split(/[\s,]+/).every((id) => id === "" || /^\d+$/.test(id))) {
    return "Enter Discord ids: digits, separated by commas.";
  }
  if (kind === "json") {
    try {
      JSON.parse(input);
    } catch {
      return "This is not valid JSON.";
    }
  }
  return undefined;
}

/** The value to send for an input; `null` removes the setting so the default applies. */
export function fromInput(input: string | boolean, kind: FieldKind): unknown {
  if (kind === "bool") {
    return input === true;
  }
  const text = typeof input === "string" ? input.trim() : "";
  if (text === "") {
    return null;
  }
  switch (kind) {
    case "number":
      return Number(text);
    case "list":
      return text
        .split(/[,\n]+/)
        .map((item) => item.trim())
        .filter(Boolean);
    case "ids":
      return text.split(/[\s,]+/).filter(Boolean);
    case "json":
      return JSON.parse(text) as unknown;
    default:
      return text;
  }
}

/** The inputs of `fields` as `values` has them now. */
export function initialInputs(
  fields: readonly FieldSpec[],
  values: unknown,
): Record<string, string | boolean> {
  return Object.fromEntries(
    fields
      .filter((f) => f.kind !== "secret")
      .map((f) => [f.path, toInput(getIn(values, f.path), f.kind)]),
  );
}

/**
 * The changes between `initial` and `inputs`, plus every secret that is set or cleared,
 * shaped like `hub.toml`.
 */
export function buildChanges(
  fields: readonly FieldSpec[],
  initial: Record<string, string | boolean>,
  inputs: Record<string, string | boolean>,
  secrets: Record<string, SecretChange>,
): Changes {
  const changes: Changes = {};
  for (const field of fields) {
    if (field.kind === "secret") {
      const change = secrets[field.path];
      if (change && !("keep" in change)) {
        setIn(changes, field.path, change);
      }
      continue;
    }
    const now = inputs[field.path];
    if (now !== undefined && now !== initial[field.path]) {
      setIn(changes, field.path, fromInput(now, field.kind));
    }
  }
  return changes;
}
