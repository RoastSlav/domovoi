import type { AckRule, AckRuleInput, Predicate } from "../../api/types";
import { opLabel } from "../rules/AddRuleForm";

/** The check the agent reports for a successful SSH login. */
export const SSH_CHECK = "security.ssh_login";

/** Where a login comes from, with the words in the form. `range` needs an address. */
export const FROM: [string, string][] = [
  ["anywhere", "Anywhere"],
  ["lan", "Local network"],
  ["range", "This address or range"],
];

/** How long a rule lives, with the words in the form; `""` is always. */
export const FOR: [string, string][] = [
  ["60", "1 hour"],
  ["1440", "1 day"],
  ["10080", "1 week"],
  ["", "Always"],
];

/** What the add form holds. */
export interface Fields {
  cidr: string;
  from: string;
  life: string;
  note: string;
  user: string;
}

/** The add form starts with: anyone, from anywhere, for one hour. */
export const EMPTY: Fields = {
  user: "",
  from: "anywhere",
  cidr: "",
  life: "60",
  note: "Expected SSH login",
};

/** What is missing from the add form, by field. */
export interface LoginProblems {
  cidr?: string;
  note?: string;
}

/** The rule the form describes, as a request body, or what to fix in which field. */
export function loginBody(host: string, fields: Fields): { body: AckRuleInput } | LoginProblems {
  const note = fields.note.trim();
  if (note === "") {
    return { note: "Type why the login is expected." };
  }
  const predicates: Predicate[] = [];
  if (fields.from === "lan") {
    predicates.push({ key: "network", op: "equals", value: "lan" });
  }
  if (fields.from === "range") {
    const cidr = fields.cidr.trim();
    if (cidr === "") {
      return { cidr: "Type the address, or the range in CIDR form, for example 192.168.1.0/24." };
    }
    predicates.push({ key: "ip", op: "in_network", value: cidr });
  }
  return {
    body: {
      host,
      check: SSH_CHECK,
      target_pattern: fields.user.trim() === "" ? null : fields.user.trim(),
      predicates,
      note,
      expires_in_minutes: fields.life === "" ? null : Number(fields.life),
    },
  };
}

/**
 * The body of the one-hour window opened from the page: logins on `host` for one hour.
 * Local network logins are the default; internet logins are included only when asked for,
 * so one click never silences a login from outside the network.
 */
export function loginWindow(host: string, internet: boolean): AckRuleInput {
  return {
    host,
    check: SSH_CHECK,
    target_pattern: null,
    predicates: internet ? [] : [{ key: "network", op: "equals", value: "lan" }],
    note: "Login window opened from the web UI",
    expires_in_minutes: 60,
  };
}

/** Who the rule expects, in words: `alice`, or `anyone` when it has no target. */
export function userWords(rule: AckRule): string {
  return rule.target_pattern ?? "anyone";
}

/** Where logins come from, in words: `the local network`, `the range 192.168.1.0/24`, `anywhere`. */
export function fromWords(predicates: Predicate[]): string {
  for (const predicate of predicates) {
    if (predicate.key === "network" && predicate.op === "equals") {
      return predicate.value === "lan" ? "the local network" : `the ${predicate.value} network`;
    }
    if (predicate.key === "ip" && predicate.op === "in_network") {
      return predicate.value.includes("/")
        ? `the range ${predicate.value}`
        : `the address ${predicate.value}`;
    }
    if (predicate.key === "ip" && predicate.op === "equals") {
      return `the address ${predicate.value}`;
    }
  }
  if (predicates.length === 0) {
    return "anywhere";
  }
  return predicates.map((p) => `${p.key} ${opLabel(p.op)} ${p.value}`).join("; ");
}

/** An expected login that covers `host`: its host is `host`, or null for any host. */
export function expectedOn(rules: readonly AckRule[], host: string): AckRule[] {
  return rules.filter(
    (rule) => (rule.host === host || rule.host === null) && rule.check === SSH_CHECK,
  );
}

/** The open login window of `host` that expires soonest: a rule with no target and an
 * expiry after `now` (an ISO time), or `undefined` when there is none. */
export function openWindow(
  rules: readonly AckRule[],
  host: string,
  now: string,
): (AckRule & { expires_at: string }) | undefined {
  return expectedOn(rules, host)
    .filter(
      (rule): rule is AckRule & { expires_at: string } =>
        rule.target_pattern === null &&
        typeof rule.expires_at === "string" &&
        rule.expires_at > now,
    )
    .sort((a, b) => (a.expires_at < b.expires_at ? -1 : 1))[0];
}
