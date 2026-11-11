import { useDeleteAckRule } from "../../api/hooks/rules";
import type { AckRule } from "../../api/types";
import { ConfirmDialog, Stack, Text, useActionFeedback } from "../../components";
import { hostWords, matchPhrase } from "./phrase";

/**
 * Asks before deleting a known condition: names the rule and says what changes. The result
 * is a toast once the dialog has closed; a failure stays in the dialog.
 *
 * There is no Undo: a rule's expiry is a number of minutes from now when it is created, so
 * creating it again would not restore it exactly.
 */
export function DeleteRuleDialog({ rule, onClose }: { rule: AckRule; onClose: () => void }) {
  const remove = useActionFeedback(useDeleteAckRule(), {
    success: `Known condition for ${rule.check} deleted`,
    error: "delete the known condition",
    errorDisplay: "inline",
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
