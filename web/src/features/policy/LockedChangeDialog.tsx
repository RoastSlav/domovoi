import type { PolicyPreview } from "../../api/types";
import { Button, InlineMessage, PasswordPrompt, Stack, StatusPill, Text } from "../../components";
import type { DescribedError } from "../../design/messages";
import { lockChangeMeta } from "../../design/tone";
import { LOCK_HELP } from "./policyText";
import { SimulationSummary } from "./SimulationSummary";

/**
 * Shows what a save would change in locked rules, what it would do to recorded decisions, and
 * asks for the password. Nothing is sent until the password is confirmed.
 * @param preview The review the save is tied to.
 * @param busy The save is running.
 * @param passwordError A wrong password or a lockout, in words, shown under the field.
 * @param problem Any other failure of the save, in the hub's words.
 * @param canCheckAgain Offers a fresh review when the hub says the old one is out of date.
 * @param onConfirm Receives the password; the caller never stores it.
 * @param onCheckAgain Closes the dialog and reviews the rules again.
 */
export function LockedChangeDialog({
  preview,
  busy,
  canCheckAgain,
  passwordError,
  problem,
  onConfirm,
  onCancel,
  onCheckAgain,
}: {
  busy: boolean;
  /** The hub says the review is out of date. */
  canCheckAgain: boolean;
  onCancel: () => void;
  onCheckAgain: () => void;
  onConfirm: (password: string) => void;
  passwordError: string | undefined;
  preview: PolicyPreview;
  problem: DescribedError | undefined;
}) {
  const widening = preview.simulation?.newly_allowed ?? 0;
  return (
    <PasswordPrompt
      title="Change a locked rule"
      reason="Enter your password to save these changes."
      confirmLabel="Save draft"
      busy={busy}
      error={passwordError}
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <Stack gap={3}>
        <Text muted block>
          {LOCK_HELP}
        </Text>
        <ul>
          {preview.locked_changes.map((change) => (
            <li key={`${change.rule}:${change.change}`}>
              <StatusPill size="sm" meta={lockChangeMeta(change.change)} />{" "}
              <Text strong mono>
                {change.rule}
              </Text>
            </li>
          ))}
        </ul>
        {preview.simulation && (
          <>
            <SimulationSummary summary={preview.simulation} label="Effect on recorded decisions" />
            {widening > 0 && (
              <InlineMessage tone="warning" title="Some actions would run without asking">
                {widening === 1
                  ? "1 recorded decision would become Allow automatically."
                  : `${widening} recorded decisions would become Allow automatically.`}
              </InlineMessage>
            )}
          </>
        )}
        {problem && (
          <InlineMessage
            tone="danger"
            title={problem.title}
            action={
              canCheckAgain && (
                <Button size="sm" onClick={onCheckAgain}>
                  Check again
                </Button>
              )
            }
          >
            {problem.detail}
          </InlineMessage>
        )}
      </Stack>
    </PasswordPrompt>
  );
}
