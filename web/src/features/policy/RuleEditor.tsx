import { useState } from "react";
import { useHostNames } from "../../api/hooks/hosts";
import type { PolicyRule } from "../../api/types";
import {
  Button,
  Card,
  Field,
  InlineMessage,
  SegmentedControl,
  Stack,
  Switch,
  Text,
  Textarea,
  TextInput,
  type Segment,
} from "../../components";
import { policyOutcomeMeta, ruleMarkMeta } from "../../design/tone";
import { ConditionFields } from "./ConditionFields";
import { LOCK_HELP, SHADOW_HELP } from "./policyText";
import { useTimeWindowZone } from "./policyZone";
import { RuleScopeFields } from "./RuleScopeFields";
import {
  fromDto,
  newRule,
  toDto,
  validateRule,
  type FormOutcome,
  type Problems,
  type RuleForm,
} from "./ruleModel";
import { conditionText, scopeText } from "./ruleText";
import styles from "./draft.module.css";

const OUTCOMES: Segment<FormOutcome>[] = (["deny", "require_approval", "allow"] as const).map(
  (value) => ({ value, label: policyOutcomeMeta(value).label }),
);

const NO_PROBLEMS: Problems = { conditions: {} };

/**
 * The form for one rule of the draft. Apply writes the rule into the working copy on this
 * page; it does not save the draft.
 * @param ruleId The id of the rule to edit, or `new`.
 * @param rules The working copy's rules.
 * @param onApply Receives the rule, and the id it replaces when it was edited.
 * @param onClose Closes the editor.
 */
export function RuleEditor({
  ruleId,
  rules,
  onApply,
  onClose,
}: {
  onApply: (rule: PolicyRule, replaces: string | undefined) => void;
  onClose: () => void;
  ruleId: string;
  rules: readonly PolicyRule[];
}) {
  const existing = ruleId === "new" ? undefined : rules.find((r) => r.id === ruleId);
  const [form, setForm] = useState<RuleForm>(() =>
    existing ? fromDto(existing) : newRule(rules.map((r) => r.id)),
  );
  const [attempted, setAttempted] = useState(false);
  const zone = useTimeWindowZone();
  const hostNames = useHostNames();
  const otherIds = rules.filter((r) => r.id !== existing?.id).map((r) => r.id);
  const problems = attempted ? validateRule(form, otherIds) : NO_PROBLEMS;
  const change = (patch: Partial<RuleForm>) => setForm((now) => ({ ...now, ...patch }));
  const built = toDto(form, otherIds);

  const apply = () => {
    setAttempted(true);
    if (!built.ok) return;
    onApply(built.rule, existing?.id);
    onClose();
  };
  const plain = built.ok
    ? [
        scopeText(built.rule, hostNames),
        ...(built.rule.conditions ?? []).map((c) => `${conditionText(c, zone)}.`),
      ].join(" ")
    : "Fix the fields marked above to see what this rule says.";

  if (form.unsupported) {
    return (
      <Card label="Rule">
        <Stack gap={4}>
          <InlineMessage tone="warning" title="This rule uses something this page does not know">
            It is kept as it is. Edit it with a newer version of the interface.
          </InlineMessage>
          <Text strong mono block>
            {form.unsupported.id}
          </Text>
          <Text mono block>
            {JSON.stringify(form.unsupported, null, 2)}
          </Text>
          <div className={styles.editorActions}>
            <Button variant="primary" disabledReason="This page cannot change this rule.">
              Apply to draft
            </Button>
            <Button onClick={onClose}>Cancel</Button>
          </div>
        </Stack>
      </Card>
    );
  }

  return (
    <Card label="Rule">
      <Stack gap={4}>
        <Field label="Rule name" hint="Shown in decisions and reports." error={problems.id}>
          <TextInput mono value={form.id} onChange={(e) => change({ id: e.target.value })} />
        </Field>
        <SegmentedControl
          label="Outcome"
          value={form.outcome}
          options={OUTCOMES}
          onChange={(outcome) =>
            change({ outcome, locked: outcome === "deny" ? form.locked : false })
          }
        />
        {form.outcome === "deny" && (
          <>
            <Switch
              label={ruleMarkMeta("locked_deny").label}
              hint={LOCK_HELP}
              checked={form.locked}
              onChange={(locked) => change({ locked })}
            />
            <Text muted block>
              Saving a change to a locked rule asks for your password and shows what it would
              change.
            </Text>
          </>
        )}
        <Switch
          label="Enabled"
          hint="Turn it off to keep the rule but ignore it."
          checked={form.enabled}
          onChange={(enabled) => change({ enabled })}
        />
        <Switch
          label={ruleMarkMeta("shadow").label}
          hint={SHADOW_HELP}
          checked={form.shadow}
          onChange={(shadow) => change({ shadow })}
        />
        <RuleScopeFields form={form} problems={problems} onChange={change} />
        <ConditionFields
          conditions={form.conditions}
          problems={problems.conditions}
          zone={zone}
          onChange={(conditions) => change({ conditions })}
        />
        <Field label="Note" hint={`${form.note.length} of 500 characters.`} error={problems.note}>
          <Textarea rows={3} value={form.note} onChange={(e) => change({ note: e.target.value })} />
        </Field>
        <Text muted block>
          {`In plain words: ${plain}`}
        </Text>
        <div className={styles.editorActions}>
          <Button variant="primary" onClick={apply}>
            Apply to draft
          </Button>
          <Button onClick={onClose}>Cancel</Button>
        </div>
      </Stack>
    </Card>
  );
}
