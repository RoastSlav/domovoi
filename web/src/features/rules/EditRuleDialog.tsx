import { useId, useState, type FormEvent } from "react";
import { useUpdateAckRule } from "../../api/hooks/rules";
import type { AckRule, AckRulePatch } from "../../api/types";
import {
  Button,
  Dialog,
  ErrorDetail,
  Field,
  Grid,
  InlineMessage,
  Select,
  Stack,
  TextInput,
  useActionFeedback,
} from "../../components";
import { HostField } from "./HostField";
import { EXPIRIES } from "./phrase";
import { MAX_NOTE } from "./ruleForm";

/** The option that leaves the expiry as it is. */
const KEEP = "keep";

/**
 * The dialog that edits a known condition: its host, target, note and expiry. The check stays;
 * to change that, delete the rule and add another. The hub checks the change like a new rule
 * and the failure stays in the dialog.
 */
export function EditRuleDialog({ rule, onClose }: { rule: AckRule; onClose: () => void }) {
  const formId = useId();
  const update = useActionFeedback(useUpdateAckRule(), {
    success: "Known condition saved",
    error: "save the known condition",
    errorDisplay: "inline",
  });
  const [host, setHost] = useState(rule.host ?? "");
  const [target, setTarget] = useState(rule.target_pattern ?? "");
  const [note, setNote] = useState(rule.note);
  const [expiry, setExpiry] = useState(KEEP);
  const [problem, setProblem] = useState<string>();

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (note.trim() === "" || note.length > MAX_NOTE) {
      setProblem(`Give a note of 1 to ${MAX_NOTE} characters.`);
      return;
    }
    const patch: AckRulePatch = {
      host: host === "" ? null : host,
      target_pattern: target.trim() === "" ? null : target.trim(),
      note: note.trim(),
    };
    if (expiry !== KEEP) {
      patch.expires_at = null;
      if (expiry !== "") {
        patch.expires_in_minutes = Number(expiry);
        delete patch.expires_at;
      }
    }
    update.mutate({ id: rule.id, patch }, { onSuccess: onClose });
  };

  return (
    <Dialog
      title={`Edit ${rule.check}`}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" type="submit" form={formId} loading={update.isPending}>
            Save
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={onSubmit} noValidate>
        <Stack gap={4}>
          <HostField value={host} onChange={setHost} />
          <Field
            label="Target"
            hint="Optional. Use * at the start or end to match more. Empty matches any target."
          >
            <TextInput mono value={target} onChange={(event) => setTarget(event.target.value)} />
          </Field>
          <Grid min="sm" gap={4}>
            <Field label="Lasts" hint="Counted from now. Keep leaves the expiry as it is.">
              <Select value={expiry} onChange={(event) => setExpiry(event.target.value)}>
                <option value={KEEP}>Keep as it is</option>
                {EXPIRIES.map(([value, label]) => (
                  <option key={label} value={value}>
                    {label}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Note" required hint="Why this is acceptable." error={problem}>
              <TextInput
                value={note}
                onChange={(event) => {
                  setNote(event.target.value);
                  setProblem(undefined);
                }}
              />
            </Field>
          </Grid>
          {update.error && (
            <InlineMessage tone="danger" title={update.error.title}>
              <ErrorDetail error={update.error} />
            </InlineMessage>
          )}
        </Stack>
      </form>
    </Dialog>
  );
}
