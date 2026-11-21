import type { ReactNode } from "react";
import type { PolicyRule } from "../../api/types";
import { Button, PageHeader, Stack, Visible } from "../../components";
import { DraftBanner } from "./DraftBanner";
import { RuleEditor } from "./RuleEditor";
import type { RuleIssue } from "./ruleIssues";
import styles from "./policy.module.css";

/**
 * Editing one rule is a screen of its own: the rule's form in one column, with a way back to
 * the list. The list, the checks and the draft's details are not drawn while it is open.
 * @param ruleId The rule to edit, or `new`.
 * @param rules The working copy's rules.
 * @param elsewhere The stored draft moved on while edits were open.
 * @param issues The hub's problems with the rules.
 * @param onApply Writes the rule into the working copy.
 * @param onBack Returns to the list.
 * @param onReload Starts the reload, which asks first.
 * @param children Page-level parts that must stay mounted, such as the leave guard.
 */
export function RuleScreen({
  ruleId,
  rules,
  elsewhere,
  issues,
  onApply,
  onBack,
  onReload,
  children,
}: {
  children?: ReactNode;
  elsewhere: boolean;
  issues: readonly RuleIssue[];
  onApply: (rule: PolicyRule, replaces: string | undefined) => void;
  onBack: () => void;
  onReload: () => void;
  ruleId: string;
  rules: readonly PolicyRule[];
}) {
  return (
    <Stack gap={5}>
      <PageHeader
        title={ruleId === "new" ? "Add a rule" : `Edit rule ${ruleId}`}
        intro="Say what should happen, then what it applies to. Applying adds the rule to the draft; saving the draft is a separate step."
        actions={
          <Button variant="ghost" icon="chevron-left" onClick={onBack}>
            Back to the rules
          </Button>
        }
      />
      <DraftBanner elsewhere={elsewhere} issues={issues} onReload={onReload} />
      <Visible category="P1" id="draft.editor">
        <div className={styles.focusedEditor}>
          <RuleEditor
            key={ruleId}
            ruleId={ruleId}
            rules={rules}
            onApply={onApply}
            onClose={onBack}
          />
        </div>
      </Visible>
      {children}
    </Stack>
  );
}
