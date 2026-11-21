/** One problem the hub reported against a rule set. */
export interface RuleIssue {
  path: string;
  message: string;
  severity?: string;
}

/**
 * Sorts reported problems onto the rule rows they belong to.
 * @param issues Problems with a path such as `rules.<id>`.
 * @returns Messages by rule id, and the messages that name no rule.
 */
export function issuesByRule(issues: readonly RuleIssue[]): {
  byRule: Map<string, string[]>;
  rest: string[];
} {
  const byRule = new Map<string, string[]>();
  const rest: string[] = [];
  for (const issue of issues) {
    const id = issue.path.startsWith("rules.") ? issue.path.slice("rules.".length) : "";
    if (id === "") {
      rest.push(issue.message);
    } else {
      byRule.set(id, [...(byRule.get(id) ?? []), issue.message]);
    }
  }
  return { byRule, rest };
}
