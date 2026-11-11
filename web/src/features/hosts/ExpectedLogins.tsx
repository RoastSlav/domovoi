import { useState } from "react";
import { useDeleteAckRule } from "../../api/hooks/rules";
import type { AckRule } from "../../api/types";
import {
  Button,
  ConfirmDialog,
  DataTable,
  EmptyState,
  RelativeTime,
  Text,
  useActionFeedback,
  type Column,
} from "../../components";
import { fromWords, userWords } from "./sshRules";

/** Delete one expected login, asking first. */
function RemoveButton({ rule }: { rule: AckRule }) {
  const [asking, setAsking] = useState(false);
  const remove = useActionFeedback(useDeleteAckRule(), {
    success: "Expected login removed",
    error: "remove the expected login",
    errorDisplay: "inline",
  });
  const close = () => {
    setAsking(false);
    remove.reset();
  };
  const words = `${userWords(rule)} from ${fromWords(rule.predicates)}`;
  return (
    <>
      <Button
        size="sm"
        variant="ghost"
        icon="trash"
        aria-label={`Remove the expected login ${words}`}
        onClick={() => setAsking(true)}
      >
        Remove
      </Button>
      {asking && (
        <ConfirmDialog
          title={`Remove the expected login on ${rule.host ?? "any host"}?`}
          consequence={
            <>
              <strong>{words}</strong> starts alerting again. A login that already matched stays in
              the history.
            </>
          }
          confirmLabel="Remove"
          tone="danger"
          busy={remove.isPending}
          error={remove.error}
          onConfirm={() => remove.mutate(rule.id, { onSuccess: close })}
          onCancel={close}
        >
          <p>{`“${rule.note}”`}</p>
        </ConfirmDialog>
      )}
    </>
  );
}

const COLUMNS: Column<AckRule>[] = [
  {
    key: "user",
    label: "User",
    cell: (rule) => <Text strong>{userWords(rule)}</Text>,
  },
  {
    key: "from",
    label: "From",
    cell: (rule) => `${fromWords(rule.predicates)}${rule.host === null ? " on any host" : ""}`,
  },
  {
    key: "lasts",
    label: "Expires",
    cell: (rule) =>
      rule.expires_at ? <RelativeTime at={rule.expires_at} /> : <Text muted>never</Text>,
  },
  {
    key: "actions",
    label: "Actions",
    hideLabel: true,
    cell: (rule) => <RemoveButton rule={rule} />,
  },
];

/**
 * The logins marked as expected on this host, including rules for any host, which silence
 * logins here too. With none, it says so and where to add one.
 */
export function ExpectedLogins({ host, rules }: { host: string; rules: readonly AckRule[] }) {
  return (
    <DataTable
      caption={`Expected logins on ${host}`}
      columns={COLUMNS}
      rows={rules}
      rowKey={(rule) => rule.id}
      empty={
        <EmptyState icon="user" title={`No logins are marked as expected on ${host}`}>
          <p>
            Every login there raises an incident. Add one for a person or a machine you expect, so
            it is recorded without alerting.
          </p>
        </EmptyState>
      }
    />
  );
}
