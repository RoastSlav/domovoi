import type { Predicate, PredicateOp } from "../../api/types";

/** A condition's operator with the plain words the page uses for it. */
export const OPS: readonly (readonly [PredicateOp, string])[] = [
  ["equals", "is"],
  ["contains", "contains"],
  ["at_most", "is at most"],
  ["at_least", "is at least"],
  ["in_network", "is in the network"],
];

/** How long a rule lives, in minutes, with the words in the form; `""` means it never expires. */
export const EXPIRIES: readonly (readonly [string, string])[] = [
  ["60", "1 hour"],
  ["1440", "1 day"],
  ["10080", "1 week"],
  ["43200", "30 days"],
  ["", "Never"],
];

/** The words for an operator, for example "is at most"; an unknown one shows as it is. */
export function opLabel(op: PredicateOp): string {
  return OPS.find(([value]) => value === op)?.[1] ?? op;
}

/** An identifier such as `security.ssh_login` as words: "security ssh login". */
export function identifierWords(identifier: string): string {
  return identifier.replace(/[._]+/g, " ").trim();
}

/** One condition as a phrase, for example "used percent is at most 90". */
export function conditionPhrase(condition: Predicate): string {
  return `${identifierWords(condition.key)} ${opLabel(condition.op)} ${condition.value}`;
}

/** What a rule matches: the check, the target and the conditions, as one capitalised phrase. */
export interface Matcher {
  check: string;
  predicates: readonly Predicate[];
  target?: string | null | undefined;
}

/**
 * What a rule matches in words, for example "Disk usage on /srv, when used percent is at
 * most 90". The table and the form's preview both use it, so they always agree.
 */
export function matchPhrase({ check, target, predicates }: Matcher): string {
  const subject = identifierWords(check);
  const where = target ? ` on ${target}` : "";
  const when =
    predicates.length > 0 ? `, when ${predicates.map(conditionPhrase).join(" and ")}` : "";
  const phrase = `${subject}${where}${when}`;
  return phrase.charAt(0).toUpperCase() + phrase.slice(1);
}

/** The host a rule is limited to, in words. */
export function hostWords(host: string | null | undefined): string {
  return host ? host : "any host";
}

/** The form's expiry choice as the end of a sentence: "for 1 hour". */
function expiryClause(minutes: string): string {
  const label = EXPIRIES.find(([value]) => value === minutes)?.[1];
  if (minutes === "" || label === undefined) {
    return "until you delete the rule";
  }
  return `for ${label}`;
}

/** What the add form holds, for the preview sentence. */
export interface PreviewParts extends Matcher {
  expiry: string;
  host: string;
}

/**
 * The rule the form describes, as a sentence. Conditions that are not filled in yet are left
 * out, so the sentence follows what the person has typed so far.
 */
export function previewSentence(parts: PreviewParts): string {
  const filled = parts.predicates
    .map((condition) => ({
      ...condition,
      key: condition.key.trim(),
      value: condition.value.trim(),
    }))
    .filter((condition) => condition.key !== "" && condition.value !== "");
  const matcher = matchPhrase({
    check: parts.check.trim(),
    target: parts.target?.trim(),
    predicates: filled,
  });
  return `On ${hostWords(parts.host)}: ${matcher}. Matching incidents are kept in the history and never notify, ${expiryClause(parts.expiry)}.`;
}
