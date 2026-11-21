import type { PolicyRule } from "../../api/types";
import { StatusPill } from "../../components";
import { policyOutcomeMeta, ruleMarkMeta } from "../../design/tone";
import styles from "./policy.module.css";

/**
 * A rule's or decision's outcome as a pill.
 * @param outcome `allow`, `require_approval`, `deny`, `unavailable` or `error`.
 */
export function OutcomePill({ outcome }: { outcome: string }) {
  return <StatusPill meta={policyOutcomeMeta(outcome)} />;
}

/**
 * The marks on a rule: Locked Deny, Shadow and Disabled. A plain rule shows none.
 * @param rule The rule to mark.
 */
export function RuleMarks({ rule }: { rule: PolicyRule }) {
  const lockedDeny = rule.locked && rule.outcome === "deny";
  if (!lockedDeny && !rule.shadow && rule.enabled) return null;
  return (
    <span className={styles.marks}>
      {lockedDeny && <StatusPill size="sm" meta={ruleMarkMeta("locked_deny")} />}
      {rule.shadow && <StatusPill size="sm" meta={ruleMarkMeta("shadow")} />}
      {!rule.enabled && <StatusPill size="sm" meta={ruleMarkMeta("disabled")} />}
    </span>
  );
}
