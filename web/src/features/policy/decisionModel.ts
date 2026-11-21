/** One condition of a stored decision, in one shape whatever the hub wrote. */
export type ConditionView =
  | { kind: "arg_constraint"; arg: string; op: "int_at_least" | "int_at_most"; value: number }
  | { kind: "time_window"; start: string; end: string }
  | { kind: "requires_fact"; fact: string; expected: boolean }
  | { kind: "max_rate"; maxPerHour: number }
  | { kind: "max_ttl"; seconds: number | null; raw: string | null }
  | { kind: "unknown"; raw: string };

/** A warning attached to a stored decision. */
export type WarningView =
  | { kind: "dangerous_exception"; allow: string; deny: string }
  | { kind: "safety_set_unverified" }
  | { kind: "management_paths_unprotected"; paths: string[] }
  | { kind: "unknown"; raw: string };

/** A stored decision document, narrowed field by field. */
export interface DecisionDoc {
  /** `allow`, `require_approval`, `deny`, `unavailable` or text from a newer hub. */
  outcome: string;
  unavailableReason: string | null;
  reason: string;
  matched: string[];
  winners: string[];
  locked: string[];
  overridden: { broader: string; by: string }[];
  wouldHaveBeen: DecisionDoc | null;
  constraints: {
    maxPerHour: number | null;
    maxTtlSeconds: number | null;
    targets: string[] | null;
    conditions: ConditionView[];
  };
  limits: string[];
  facts: { fact: string; status: string }[];
  typedConfirmation: boolean;
  warnings: WarningView[];
  ceilingHash: string | null;
}

/** What a stored decision says: a document or the code of an evaluation error. */
export type DecisionView = { kind: "error"; code: string } | { kind: "decision"; doc: DecisionDoc };

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const list = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const strings = (v: unknown): string[] => list(v).filter((x): x is string => typeof x === "string");

function rawText(v: unknown): string {
  try {
    return JSON.stringify(v) ?? String(v);
  } catch {
    return String(v);
  }
}

const ISO = /^P(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?)$/i;
const FRIENDLY = /^(?:(\d+)\s*h)?\s*(?:(\d+)\s*m)?\s*(?:(\d+)\s*s)?$/i;

/**
 * Reads a duration written as ISO 8601 or as friendly text.
 * @param text For example `PT1H30M` or `1h 30m`.
 * @returns Whole seconds, or null when the text is neither form.
 */
export function parseDuration(text: string): number | null {
  const t = text.trim();
  if (t === "" || /^PT?$/i.test(t)) return null;
  const m = ISO.exec(t) ?? FRIENDLY.exec(t);
  if (!m) return null;
  const [h, min, s] = [m[1], m[2], m[3]].map((x) => (x === undefined ? 0 : Number(x)));
  return (h ?? 0) * 3600 + (min ?? 0) * 60 + (s ?? 0);
}

const pad = (n: number): string => String(n).padStart(2, "0");

function clock(v: unknown): string | null {
  if (typeof v === "string") return v;
  if (isObj(v)) {
    const h = num(v.hour);
    const m = num(v.minute);
    if (h !== null && m !== null) return `${pad(h)}:${pad(m)}`;
  }
  return null;
}

function ttl(c: Obj): ConditionView {
  const v = c.max_ttl ?? c.max_ttl_secs;
  if (typeof v === "number") return { kind: "max_ttl", seconds: v, raw: null };
  if (typeof v === "string") return { kind: "max_ttl", seconds: parseDuration(v), raw: v };
  if (isObj(v) && num(v.secs) !== null) return { kind: "max_ttl", seconds: num(v.secs), raw: null };
  return { kind: "unknown", raw: rawText(c) };
}

function conditionOf(raw: unknown): ConditionView {
  if (!isObj(raw)) return { kind: "unknown", raw: rawText(raw) };
  const unknown: ConditionView = { kind: "unknown", raw: rawText(raw) };
  switch (raw.kind) {
    case "arg_constraint": {
      const arg = str(raw.arg);
      const value = num(raw.value);
      if (arg === null || value === null || (raw.op !== "int_at_least" && raw.op !== "int_at_most"))
        return unknown;
      return { kind: "arg_constraint", arg, op: raw.op, value };
    }
    case "time_window": {
      const start = clock(raw.start);
      const end = clock(raw.end);
      return start !== null && end !== null ? { kind: "time_window", start, end } : unknown;
    }
    case "requires_fact":
      return str(raw.fact) !== null && typeof raw.expected === "boolean"
        ? { kind: "requires_fact", fact: raw.fact as string, expected: raw.expected }
        : unknown;
    case "max_rate": {
      const n = num(raw.max_per_hour);
      return n === null ? unknown : { kind: "max_rate", maxPerHour: n };
    }
    case "max_ttl":
      return ttl(raw);
    default:
      return unknown;
  }
}

function warningOf(raw: unknown): WarningView {
  if (isObj(raw)) {
    if (raw.kind === "dangerous_exception")
      return {
        kind: "dangerous_exception",
        allow: str(raw.allow) ?? "",
        deny: str(raw.deny) ?? "",
      };
    if (raw.kind === "safety_set_unverified") return { kind: "safety_set_unverified" };
    if (raw.kind === "management_paths_unprotected")
      return { kind: "management_paths_unprotected", paths: strings(raw.paths) };
  }
  return { kind: "unknown", raw: rawText(raw) };
}

function docOf(raw: Obj, depth: number): DecisionDoc {
  const outcome = isObj(raw.outcome) ? raw.outcome : {};
  const constraints = isObj(raw.constraints) ? raw.constraints : {};
  const would =
    depth < 4 && isObj(raw.would_have_been) ? docOf(raw.would_have_been, depth + 1) : null;
  return {
    outcome: str(outcome.kind) ?? str(raw.outcome) ?? "unknown",
    unavailableReason: str(outcome.reason),
    reason: str(raw.reason) ?? "",
    matched: strings(raw.matched),
    winners: strings(raw.winners),
    locked: strings(raw.locked),
    overridden: list(raw.overridden).flatMap((o) =>
      isObj(o) ? [{ broader: str(o.broader) ?? "", by: str(o.by) ?? "" }] : [],
    ),
    wouldHaveBeen: would,
    constraints: {
      maxPerHour: num(constraints.max_per_hour),
      maxTtlSeconds: num(constraints.max_ttl_secs),
      targets: Array.isArray(constraints.targets) ? strings(constraints.targets) : null,
      conditions: list(constraints.conditions).map(conditionOf),
    },
    limits: strings(raw.hard_limits_applied),
    facts: list(raw.facts_used).flatMap((f) =>
      isObj(f) ? [{ fact: str(f.fact) ?? "", status: str(f.status) ?? "unknown" }] : [],
    ),
    typedConfirmation: raw.typed_confirmation === "typed",
    warnings: list(raw.warnings).map(warningOf),
    ceilingHash: str(raw.ceiling_hash),
  };
}

/**
 * Reads a stored decision, whatever it holds.
 * @param raw The untyped JSON the hub stored; never throws.
 * @returns Null when there is none, an error view for `{error}`, else the narrowed document.
 */
export function decisionOf(raw: unknown): DecisionView | null {
  if (raw === null || raw === undefined) return null;
  if (!isObj(raw)) return { kind: "error", code: "unreadable" };
  if (typeof raw.error === "string") return { kind: "error", code: raw.error };
  return { kind: "decision", doc: docOf(raw, 0) };
}

/**
 * Reads the inputs a recorded decision could not use.
 * @param raw The untyped gap list of a stored decision.
 * @returns One entry per gap; malformed entries are skipped.
 */
export function gapsOf(raw: unknown): { input: string; reason: string }[] {
  return list(raw).flatMap((g) =>
    isObj(g) && str(g.input) !== null
      ? [{ input: g.input as string, reason: str(g.reason) ?? "" }]
      : [],
  );
}
