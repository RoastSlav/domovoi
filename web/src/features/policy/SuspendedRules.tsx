import { useState } from "react";
import { useResetRule } from "../../api/hooks/policyAutonomy";
import type { SuspendedRule } from "../../api/types";
import {
  Button,
  DataTable,
  PasswordPrompt,
  RelativeTime,
  Text,
  useActionFeedback,
} from "../../components";
import { stepUpError } from "../../design/messages";

/**
 * Rules whose circuit breaker is open: each failed restart counts, and the rule stops
 * allowing new restarts until the owner resets it with the password.
 * @param rules The suspended rules.
 */
export function SuspendedRules({ rules }: { rules: readonly SuspendedRule[] }) {
  const [rule, setRule] = useState<string | null>(null);
  const [passwordError, setPasswordError] = useState<string | undefined>(undefined);
  const reset = useActionFeedback(useResetRule(), {
    success: "Rule reset. It may allow restarts again.",
    error: "reset the rule",
    errorDisplay: "inline",
  });
  const close = () => {
    setRule(null);
    setPasswordError(undefined);
  };
  if (rules.length === 0) {
    return <Text muted>No rule is suspended.</Text>;
  }
  return (
    <>
      <DataTable
        caption="Suspended rules"
        rows={rules}
        rowKey={(row) => row.rule}
        columns={[
          { key: "rule", label: "Rule", cell: (row) => <Text mono>{row.rule}</Text> },
          { key: "failures", label: "Failures", numeric: true, cell: (row) => row.failures },
          {
            key: "since",
            label: "Since",
            cell: (row) => <RelativeTime at={row.since} />,
          },
          {
            key: "reset",
            label: "Reset",
            hideLabel: true,
            cell: (row) => (
              <Button variant="secondary" onClick={() => setRule(row.rule)}>
                Reset
              </Button>
            ),
          },
        ]}
      />
      {rule !== null && (
        <PasswordPrompt
          title="Reset this rule"
          reason={`Enter your password to let ${rule} allow restarts again. Fix what made them fail first.`}
          confirmLabel="Reset rule"
          busy={reset.isPending}
          error={passwordError ?? reset.error?.title}
          onCancel={close}
          onConfirm={(password) =>
            reset.mutate(
              { rule, password },
              {
                onSuccess: close,
                onError: (error) => setPasswordError(stepUpError(error)),
              },
            )
          }
        />
      )}
    </>
  );
}
