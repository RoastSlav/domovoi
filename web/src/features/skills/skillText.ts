/** What a risk level lets a skill do, for the Risk column's explanation. */
export const RISK_EXPLANATION =
  "Read skills only look at the host, and triage may run them on its own. Write skills change something and destructive skills can lose data. Only a signed-in person can approve a write or destructive skill; the model cannot.";

/** The order skills are listed in: the harmless first, the dangerous last. */
const RISK_ORDER = ["read", "write", "destructive"];

/** `skills` ordered by risk, then by name; an unknown risk goes last. */
export function sortByRisk<T extends { name: string; risk: string }>(skills: readonly T[]): T[] {
  const rank = (risk: string) => {
    const at = RISK_ORDER.indexOf(risk);
    return at === -1 ? RISK_ORDER.length : at;
  };
  return [...skills].sort((a, b) => rank(a.risk) - rank(b.risk) || a.name.localeCompare(b.name));
}

/** A parameter's type, from the JSON the API sends. */
export function paramType(kind: unknown): string {
  if (typeof kind !== "object" || kind === null) {
    return String(kind);
  }
  return String((kind as Record<string, unknown>)["type"]);
}

/** A parameter's limits (a range, a set of values), or an empty string when it has none. */
export function paramLimits(kind: unknown): string {
  if (typeof kind !== "object" || kind === null) {
    return "";
  }
  return Object.entries(kind as Record<string, unknown>)
    .filter(([name]) => name !== "type")
    .map(([name, value]) => `${name} ${Array.isArray(value) ? value.join(" | ") : String(value)}`)
    .join(", ");
}

/** How the hub checks the skill did its job, in one short phrase. */
export function verificationText(
  verify: { recheck: boolean; delay_secs: number } | null | undefined,
): string {
  return verify !== null && verify !== undefined && verify.recheck
    ? `Re-runs the check after ${verify.delay_secs} s`
    : "None";
}
