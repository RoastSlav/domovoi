import { useId, useState, type FormEvent } from "react";
import { useCreateAckRule } from "../../api/hooks/rules";
import {
  Button,
  Card,
  Dialog,
  ErrorDetail,
  Field,
  Grid,
  InlineMessage,
  Select,
  Stack,
  Text,
  TextInput,
  useActionFeedback,
} from "../../components";
import { ConditionRows } from "./ConditionRows";
import { HostField } from "./HostField";
import { EXPIRIES, previewSentence } from "./phrase";
import {
  NO_PROBLEMS,
  buildBody,
  emptyFields,
  mergeProblems,
  sortIssues,
  type RuleFields,
  type RuleProblems,
} from "./ruleForm";

// The words for an operator are shared with the expected logins on the host page.
export { opLabel } from "./phrase";

/** The rule as the person has described it so far, in one sentence. */
function Preview({ fields }: { fields: RuleFields }) {
  const ready = fields.check.trim() !== "";
  return (
    <Card label="Preview of the known condition">
      <Stack gap={1}>
        <Text muted strong size="xs">
          What this rule does
        </Text>
        <Text block>
          {ready
            ? previewSentence({ ...fields, predicates: fields.conditions })
            : "Type the check to see the rule described here."}
        </Text>
      </Stack>
    </Card>
  );
}

/**
 * The dialog that adds a known condition: the check it matches, the host, a target, the
 * conditions, how long it lasts and why, with a sentence that says what the rule will do.
 * A mistake marks its field or condition row, from the form or from the hub; a failure
 * stays in the dialog.
 */
export function AddRuleForm({
  defaultHost,
  onClose,
}: {
  defaultHost?: string | undefined;
  onClose: () => void;
}) {
  const formId = useId();
  const create = useActionFeedback(useCreateAckRule(), {
    success: "Known condition added",
    error: "add the known condition",
    errorDisplay: "inline",
  });
  const [fields, setFields] = useState<RuleFields>(() => emptyFields(defaultHost ?? ""));
  const [found, setFound] = useState<RuleProblems>(NO_PROBLEMS);

  const hub = sortIssues(create.error);
  const problems = mergeProblems(found, hub.problems);
  const change = (patch: Partial<RuleFields>) => {
    setFields({ ...fields, ...patch });
    setFound(NO_PROBLEMS);
    if (create.error) {
      create.reset();
    }
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    const built = buildBody(fields);
    if (!("body" in built)) {
      setFound(built);
      return;
    }
    create.mutate(built.body, { onSuccess: onClose });
  };

  return (
    <Dialog
      wide
      title="Add a known condition"
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form={formId} loading={create.isPending}>
            Add known condition
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} noValidate>
        <Stack gap={4}>
          <Grid min="sm" gap={4}>
            <Field
              label="Check"
              required
              hint="The check the incident comes from, for example security.ssh_login."
              error={problems.check}
            >
              <TextInput
                mono
                value={fields.check}
                placeholder="security.ssh_login"
                onChange={(event) => change({ check: event.target.value })}
              />
            </Field>
            <HostField
              value={fields.host}
              error={problems.host}
              onChange={(host) => change({ host })}
            />
          </Grid>
          <Field
            label="Target"
            hint="Optional. Use * at the start or end to match more, for example /var/log/*."
            error={problems.target}
          >
            <TextInput
              mono
              value={fields.target}
              placeholder="/var/log/*"
              onChange={(event) => change({ target: event.target.value })}
            />
          </Field>
          <ConditionRows
            conditions={fields.conditions}
            problems={problems.conditions}
            onChange={(conditions) => change({ conditions })}
          />
          <Grid min="sm" gap={4}>
            <Field
              label="Lasts"
              hint="After this the incidents notify again."
              error={problems.expiry}
            >
              <Select
                value={fields.expiry}
                onChange={(event) => change({ expiry: event.target.value })}
              >
                {EXPIRIES.map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Note" required hint="Why this is acceptable." error={problems.note}>
              <TextInput
                value={fields.note}
                placeholder="Known backup window"
                onChange={(event) => change({ note: event.target.value })}
              />
            </Field>
          </Grid>
          <Preview fields={fields} />
          {create.error && (
            <InlineMessage tone="danger" title={create.error.title}>
              <ErrorDetail error={{ ...create.error, issues: hub.rest }} />
            </InlineMessage>
          )}
        </Stack>
      </form>
    </Dialog>
  );
}
