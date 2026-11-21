import type { PolicyRule } from "../../api/types";
import type { ConditionView } from "./decisionModel";

type RuleCondition = NonNullable<PolicyRule["conditions"]>[number];

/**
 * Writes a length of time in plain units.
 * @param seconds Whole seconds.
 * @returns For example `45 s`, `10 min` or `1 h 30 min`.
 */
export function durationText(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const parts = [h > 0 ? `${h} h` : "", m > 0 ? `${m} min` : "", s > 0 && h === 0 ? `${s} s` : ""];
  const text = parts.filter(Boolean).join(" ");
  return text === "" ? "0 s" : text;
}

function hostText(
  rule: PolicyRule,
  hostNames: ReadonlyMap<string, string> | Record<string, string>,
): string {
  if (rule.host.kind !== "host") return "every host";
  const id = rule.host.host_id;
  const name =
    hostNames instanceof Map ? hostNames.get(id) : (hostNames as Record<string, string>)[id];
  return name ?? `Unknown host (${id.slice(0, 8)})`;
}

function capabilityText(rule: PolicyRule): string {
  const c = rule.capability;
  switch (c.kind) {
    case "effect":
      return `any ${c.effect} action`;
    case "category":
      return `${c.category} actions`;
    case "contract":
      return c.contract;
    default:
      return "any action";
  }
}

/**
 * Says what a rule applies to in plain words.
 * @param rule The rule.
 * @param hostNames Host names by id.
 * @returns For example `On web-1: container.restart on api, db.`
 */
export function scopeText(
  rule: PolicyRule,
  hostNames: ReadonlyMap<string, string> | Record<string, string>,
): string {
  const targets =
    rule.targets.kind === "exact" ? `on ${rule.targets.names.join(", ")}` : "on any target";
  return `On ${hostText(rule, hostNames)}: ${capabilityText(rule)} ${targets}.`;
}

const zonePhrase = (zone: string | undefined): string => (zone ? zone : "in the hub's time zone");

function describe(c: RuleCondition | ConditionView, zone: string | undefined): string {
  const x = c as unknown as Record<string, unknown>;
  switch (c.kind) {
    case "arg_constraint":
      return `Argument ${c.arg} is at ${c.op === "int_at_most" ? "most" : "least"} ${c.value}`;
    case "time_window":
      return `Only between ${c.start} and ${c.end} ${zonePhrase(zone)}`;
    case "requires_fact":
      return `Needs fact ${c.fact} to be known as ${c.expected ? "true" : "false"}`;
    case "max_rate":
      return `At most ${"maxPerHour" in c ? c.maxPerHour : (x.max_per_hour as number)} per hour`;
    case "max_ttl": {
      const secs = "seconds" in c ? c.seconds : (x.max_ttl_secs as number);
      const raw = "raw" in c ? c.raw : null;
      return `Lasts at most ${secs !== null ? durationText(secs) : (raw ?? "an unknown time")}`;
    }
    default:
      return `Unrecognised condition: ${"raw" in c ? String(c.raw) : JSON.stringify(c)}`;
  }
}

/**
 * Says a condition as one plain sentence.
 * @param condition A rule condition or a decision's condition view.
 * @param zone The hub's time zone; missing or empty reads as "in the hub's time zone".
 */
export function conditionText(
  condition: RuleCondition | ConditionView,
  zone: string | undefined,
): string {
  return describe(condition, zone);
}

/**
 * Says a decision's condition as one plain sentence.
 * @param view The condition from `decisionOf`.
 * @param zone The hub's time zone, if known.
 */
export function conditionViewText(view: ConditionView, zone: string | undefined): string {
  return describe(view, zone);
}

/**
 * Says who did something.
 * @param actor An actor id such as `user:ana` or `system:policy`.
 * @returns `a person`, `Domovoi`, or the text unchanged.
 */
export function actorText(actor: string): string {
  if (actor.startsWith("user:")) return "a person";
  if (actor.startsWith("system:")) return "Domovoi";
  return actor;
}
