import type { Fact } from "../../api/types";
import type { Tone } from "../../design/tone";

/** A fact older than this is shown as stale: its agent probably stopped reporting it. */
export const STALE_MS = 60 * 60 * 1000;

/** Disks and similar levels: this full is a warning, and this full is a danger. */
export const FULL_WARNING = 85;
export const FULL_DANGER = 95;

/** The fact's value as a number, or `NaN` when it is text. */
export function numberOf(fact: Fact | undefined): number {
  return fact ? Number.parseFloat(fact.value) : Number.NaN;
}

/** A number with at most one decimal and no trailing zero: `6.2`, `31`. */
export function shown(value: number): string {
  return String(Number(value.toFixed(1)));
}

/** The tone of `value` against a warning and a danger line; none while it is below both. */
export function toneAt(value: number, warning: number, danger: number): Tone | undefined {
  if (value >= danger) {
    return "danger";
  }
  return value >= warning ? "warning" : undefined;
}

/** The first fact of `facts` whose key is one of `keys`, tried in that order. */
export function findFact(facts: readonly Fact[], ...keys: string[]): Fact | undefined {
  for (const key of keys) {
    const found = facts.find((fact) => fact.key === key);
    if (found) {
      return found;
    }
  }
  return undefined;
}

/** When the newest of `facts` was observed; `undefined` for none. */
export function newest(facts: readonly Fact[]): string | undefined {
  return facts.reduce<string | undefined>(
    (latest, fact) =>
      latest === undefined || fact.observed_at > latest ? fact.observed_at : latest,
    undefined,
  );
}

/** A fact's value with its unit, `31 GiB`. */
export function withUnit(fact: Fact): string {
  return fact.unit ? `${fact.value} ${fact.unit}` : fact.value;
}

/** The tone words in a status text read: a failed device is danger, a pending repair a warning. */
export function textTone(text: string): Tone | undefined {
  if (/degraded|fault|fail|offline|unavail|critical/i.test(text)) {
    return "danger";
  }
  return /error|pending|reallocated|resync|rebuild|warn|[1-9]\d* bad/i.test(text)
    ? "warning"
    : undefined;
}
