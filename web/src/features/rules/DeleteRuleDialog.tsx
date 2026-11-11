import { useCreateAckRule, useDeleteAckRule } from "../../api/hooks/rules";
import type { AckRule, AckRuleInput } from "../../api/types";
import { ConfirmDialog, Stack, Text, useActionFeedback } from "../../components";
import { hostWords, matchPhrase } from "./phrase";

/**
 * Asks before deleting a known condition: names the rule and says what changes. The result
 * is a toast once the dialog has closed, with Undo, which creates the same rule again with
 * its original expiry; a failure stays in the dialog.
 */
/** The body that creates `rule` again, with the same expiry it had. */
function restoreInput(rule: AckRule): AckRuleInput {
  return {
    host: rule.host ?? null,
    check: rule.check,
    target_pattern: rule.target_pattern ?? null,
    predicates: rule.predicates,
    note: rule.note,
    expires_in_minutes: null,
    expires_at: rule.expires_at ?? null,
  };
}

export function DeleteRuleDialog({ rule, onClose }: { rule: AckRule; onClose: () => void }) {
  const restore = useActionFeedback(useCreateAckRule(), {
    success: `Known condition for ${rule.check} restored`,
    error: "restore the known condition",
  });
  const remove = useActionFeedback(useDeleteAckRule(), {
    success: `Known condition for ${rule.check} deleted`,
    error: "delete the known condition",
    errorDisplay: "inline",
    undo: () => restore.mutate(restoreInput(rule)),
  });
  const close = () => {
    remove.reset();
    onClose();
  };
  return (
    <ConfirmDialog
      title={`Delete the known condition for ${rule.check}?`}
      consequence="Matching incidents notify you again from now on. Incidents it already kept stay in the history."
      confirmLabel="Delete known condition"
      tone="danger"
      busy={remove.isPending}
      error={remove.error}
      onConfirm={() => remove.mutate(rule.id, { onSuccess: close })}
      onCancel={close}
    >
      <Stack gap={1}>
        <Text
          block
        >{`${matchPhrase({ ...rule, target: rule.target_pattern })} on ${hostWords(rule.host)}.`}</Text>
        <Text block muted>
          {rule.note}
        </Text>
      </Stack>
    </ConfirmDialog>
  );
}
