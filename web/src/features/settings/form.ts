import type { Changes, SecretChange } from "../../api/hooks/settings";
import { SECRET_SET, type Issue } from "../../api/types";

/** How a field is edited. */
export type FieldKind = "text" | "number" | "bool" | "select" | "list" | "ids" | "secret" | "json";

/** One editable setting. */
export interface FieldSpec {
  /** Dotted path in `hub.toml`, for example `notify.digest.time`. */
  path: string;
  label: string;
  kind: FieldKind;
  /** For `select`: the choices; an empty string means "not set". */
  options?: string[];
  /** Overrides the help text from the schema. */
  hint?: string;
  placeholder?: string;
}

/** The value at a dotted `path` of `values`. */
export function getIn(values: unknown, path: string): unknown {
  let current: unknown = values;
  for (const key of path.split(".")) {
    if (typeof current !== "object" || current === null) {
      return undefined;
    }
    current = (current as Record<string, unknown>)[key];
  }
  return current;
}

/** Sets `value` at a dotted `path` of `target`, creating tables on the way. */
export function setIn(target: Changes, path: string, value: unknown): void {
  const keys = path.split(".");
  const last = keys.pop();
  if (last === undefined) {
    return;
  }
  let current: Record<string, unknown> = target;
  for (const key of keys) {
    const next = current[key];
    if (typeof next !== "object" || next === null || Array.isArray(next)) {
      current[key] = {};
    }
    current = current[key] as Record<string, unknown>;
  }
  current[last] = value;
}

/**
 * Where the value at `path` comes from: `ui`, `file`, `env:<VAR>`, or `default` when no
 * layer set it. An entry for a parent (an array or a secret) covers what is under it.
 */
export function sourceOf(fields: Record<string, string>, path: string): string {
  let candidate = path;
  for (;;) {
    const found = fields[candidate];
    if (found) {
      return found;
    }
    const dot = candidate.lastIndexOf(".");
    if (dot < 0) {
      return "default";
    }
    candidate = candidate.slice(0, dot);
  }
}

/** The label of a locked field, or `undefined` when the UI may change it. */
export function lockedBy(source: string): string | undefined {
  if (source === "file") {
    return "Set in hub.toml";
  }
  if (source.startsWith("env:")) {
    return `Set by ${source.slice(4)}`;
  }
  return undefined;
}

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

/** Whether a secret field has a value now. */
export function isSecretSet(values: unknown, path: string): boolean {
  return getIn(values, path) === SECRET_SET;
}

/**
 * The changes between `initial` and `inputs`, plus every secret that is set or cleared,
 * shaped like `hub.toml`.
 */
export function buildChanges(
  fields: FieldSpec[],
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

/** Dotted paths of every value in `changes`. */
export function changedPaths(changes: Changes, prefix = ""): string[] {
  return Object.entries(changes).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    const isTable =
      typeof value === "object" &&
      value !== null &&
      !Array.isArray(value) &&
      !("set" in value || "clear" in value || "keep" in value);
    return isTable ? changedPaths(value as Changes, path) : [path];
  });
}

/** Whether a change widens trust, so the hub asks for the password again. */
export function isSensitive(changes: Changes): boolean {
  return changedPaths(changes).some((path) => {
    const secret = getIn(changes, path);
    const isSecret =
      typeof secret === "object" && secret !== null && ("set" in secret || "clear" in secret);
    return (
      isSecret ||
      path === "discord.allowed_user_ids" ||
      path === "web.trusted_proxies" ||
      path === "webhooks" ||
      path.startsWith("webhooks.")
    );
  });
}

/** The issues for each path; issues for a parent apply to the fields under it. */
export function issuesByPath(issues: Issue[]): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const issue of issues) {
    out.set(issue.path, [...(out.get(issue.path) ?? []), issue.message]);
  }
  return out;
}

type Schema = Record<string, unknown>;

function resolve(root: Schema, node: unknown): Schema | undefined {
  if (typeof node !== "object" || node === null) {
    return undefined;
  }
  const schema = node as Schema;
  const ref = schema.$ref;
  if (typeof ref === "string") {
    const name = ref.split("/").pop() ?? "";
    const defs = (root.$defs ?? root.definitions) as Record<string, Schema> | undefined;
    return resolve(root, defs?.[name]);
  }
  for (const key of ["anyOf", "oneOf", "allOf"]) {
    const options = schema[key];
    if (Array.isArray(options) && !schema.properties) {
      for (const option of options) {
        const found = resolve(root, option);
        if (found && (found.properties || found.description)) {
          return { description: schema.description, ...found };
        }
      }
    }
  }
  return schema;
}

/** The schema's description of the setting at `path`, for help text. */
export function describe(root: Schema | undefined, path: string): string | undefined {
  if (!root) {
    return undefined;
  }
  let node: Schema | undefined = root;
  let description: string | undefined;
  for (const key of path.split(".")) {
    const properties = node?.properties as Record<string, unknown> | undefined;
    const raw = properties?.[key] as Schema | undefined;
    const rawDescription = typeof raw?.description === "string" ? raw.description : undefined;
    node = resolve(root, raw);
    description =
      rawDescription ?? (typeof node?.description === "string" ? node.description : undefined);
  }
  return description;
}
