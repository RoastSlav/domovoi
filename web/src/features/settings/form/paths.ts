import type { Changes } from "../../../api/hooks/settings";
import { SECRET_SET, type Issue } from "../../../api/types";

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

/** Why a locked field cannot be edited here, or `undefined` when the UI may change it. */
export function lockedBy(source: string): string | undefined {
  if (source === "file") {
    return "Set in hub.toml; edit the file to change it.";
  }
  if (source.startsWith("env:")) {
    return `Set by the ${source.slice(4)} environment variable; change it there.`;
  }
  return undefined;
}

/** Whether a secret field has a value now. */
export function isSecretSet(values: unknown, path: string): boolean {
  return getIn(values, path) === SECRET_SET;
}

/** The issues for each path. */
export function issuesByPath(issues: Issue[]): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const issue of issues) {
    out.set(issue.path, [...(out.get(issue.path) ?? []), issue.message]);
  }
  return out;
}

/**
 * The field paths an issue is about. An issue for a table (`incidents`) covers every field
 * under it; one for a field covers that field. The hub names a field by its dotted path, so
 * an issue whose path matches none of `known` has no place in the form.
 */
export function fieldsForIssue(issue: Issue, known: readonly string[]): string[] {
  if (!issue.path) {
    return [];
  }
  const exact = known.filter((path) => path === issue.path);
  if (exact.length > 0) {
    return exact;
  }
  return known.filter((path) => path.startsWith(`${issue.path}.`));
}
